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
//!   served after a switch to world `c2`.
//! * A [`ResolvedTexture`] additionally carries the process-local
//!   [`TextureCatalog::serial`] of the catalog that resolved it, and
//!   [`TextureCatalog::prepare_upload`] and [`TextureCatalog::decode`]
//!   compare it: a [`ResolvedTexture`] from another catalog of the same
//!   session — a sibling opened over the same archives, which shares the
//!   generation — is refused too.
//! * [`TextureCatalog::retry_failed`] reopens only the failed archives of
//!   the same session and keeps the ones that loaded.
//!
//! # Which archive a world load opens
//!
//! A world group ships `texture.zbd` plus `rtexture2/4/6/8/<top>.zbd`, and the
//! original opens **exactly one** of them per world load. [`select_world_archive`]
//! is that decision, measured from the original rather than guessed:
//!
//! * [`texture_budget`] registers the one descriptor a world load takes — the
//!   budget in MiB and the original's r-flag — from the renderer and the
//!   `TextureMemory_HW`/`_SW` detail setting ([`TextureMemory`]). On the
//!   DirectDraw path the budget is the device's total texture memory; this
//!   project's renderer has none, so [`PROJECT_HARDWARE_TEXTURE_MIB`] is a
//!   **designed** default on top of the measured rule.
//! * [`select_world_archive`] then walks the candidate names in the measured
//!   order — `rtexture{z}.zbd` before `texture{z}.zbd` when the r-flag is set,
//!   counting down to the unnumbered `texture.zbd` — against the file names the
//!   texture search directories hold ([`TextureFiles`]), and keeps the first
//!   that exists. There is no tier-to-tier fallback for a single texture.
//! * [`texture_lookup_order`] is the order one texture **name** is looked up
//!   in: the world's archive, then the shared `rimage.zbd`, then a loose
//!   `<name>.tif` and `<name>.bmp`.
//!
//! [`TextureCatalog::open_world`] runs the selection and opens the chosen
//! archive, so no caller has to name a tier itself.
//!
//! Only the ZBD texture package is catalogued here; the conventional BMP and
//! TGA readers have no archive-member role yet. Design decisions and
//! unknowns: `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`,
//! and for the selection rule
//! `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`.

use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use cs_assets::install::sha256;
use cs_assets::vfs::{
    ContentSession, INSTALL_NAMESPACE, ResolutionTrace, SessionGeneration, WORLD_NAMESPACE,
};
use cs_assets::zbd::{ZbdContainer, ZbdError};
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedFormat, DecodedImage, Extent, ImageDescriptor,
    TextureError, ZbdStretch, ZbdTextureError, decode_base_level, read_zbd_textures,
};
use cs_formats::zbd::ZbdFamily;
use cs_types::asset_id::{AssetKey, AssetKeyError, AssetVariant, MountId, SourceSpan};
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
///
/// It is bound to the catalog that resolved it through [`Self::serial`] as
/// well as its [`SessionGeneration`]: a sibling catalog of the same session
/// shares the generation but not the serial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTexture {
    generation: SessionGeneration,
    serial: u64,
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

    /// The process-local serial of the catalog that resolved it, which is
    /// what binds it to that catalog.
    pub const fn serial(&self) -> u64 {
        self.serial
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
    /// A resolved texture handed back to this catalog did not come from it:
    /// another catalog resolved it, a sibling of the same session included.
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

/// The process-local serial of the next catalog.
///
/// A [`TextureCatalog`]'s serial is what binds a [`ResolvedTexture`] to the
/// catalog that resolved it. Two catalogs opened from the **same** session
/// over the same archives share a [`SessionGeneration`], so the generation
/// alone cannot tell them apart: without this, a payload resolved against one
/// of them would be served by the other without a word.
static NEXT_CATALOG_SERIAL: AtomicU64 = AtomicU64::new(1);

fn next_catalog_serial() -> u64 {
    NEXT_CATALOG_SERIAL.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
struct ArchiveSlot {
    key: AssetKey,
    state: Result<TextureArchive, TextureArchiveError>,
}

/// The texture archives one session makes available, and the images they
/// hold.
///
/// Two catalogs opened from the same session over the same archives stay two
/// distinct catalogs: each takes its own process-local [`Self::serial`], a
/// [`ResolvedTexture`] carries the serial of the catalog that resolved it,
/// and the upload boundary refuses a resolution whose serial is not its own.
#[derive(Debug)]
pub struct TextureCatalog {
    serial: u64,
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
            serial: next_catalog_serial(),
            generation: session.generation(),
            slots,
        }
    }

    /// This catalog's process-local serial, which every [`ResolvedTexture`]
    /// it produces carries so that a sibling catalog of the same session
    /// cannot answer for it.
    pub const fn serial(&self) -> u64 {
        self.serial
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
                serial: self.serial,
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
                serial: self.serial,
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
        // The generation alone cannot separate two catalogs opened by the
        // same session over the same archives; the serial can.
        if resolved.generation != self.generation || resolved.serial != self.serial {
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
    /// [`UploadError::Resolve`] when the texture is not from this catalog —
    /// another catalog resolved it, a sibling of this session included — or
    /// when `session` is foreign; [`UploadError::Decode`] when decoding fails.
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

/// The texture-memory detail setting `TextureMemory_SW`/`TextureMemory_HW`
/// holds: the original's enum value, not a budget.
///
/// `ZBD/zrdr.zbd`'s `detail.zrd` stores the token, the enum table sits at
/// `0x623328` and `0x530fe0` turns the value into the budget of
/// [`Self::budget_mib`]. The value — not the budget — is the identity, because
/// the video-options panel stores it (see [`TextureDetailRow`]).
///
/// Named values are `TEXMEM_MAX` (0), `TEXMEM_8MB` (3), `TEXMEM_6MB` (4),
/// `TEXMEM_4MB` (5) and `TEXMEM_2MB` (6); values 1 and 2 have no name in the
/// enum table but are reachable through [`Self::budget_mib`].
///
/// ```
/// use cs_content::textures::TextureMemory;
///
/// // `TEXMEM_8MB` is value 3, which registers a budget of 8 MiB.
/// assert_eq!(TextureMemory::EIGHT_MB.budget_mib(), 8);
/// // `TEXMEM_MAX` is 0, which starts the probe at the unnumbered name.
/// assert_eq!(TextureMemory::MAX.budget_mib(), 0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureMemory(u32);

impl TextureMemory {
    /// `TEXMEM_MAX` (0): budget 0, which starts the probe at the unnumbered
    /// name instead of a numbered tier.
    pub const MAX: Self = Self(0);
    /// `TEXMEM_8MB` (3).
    pub const EIGHT_MB: Self = Self(3);
    /// `TEXMEM_6MB` (4).
    pub const SIX_MB: Self = Self(4);
    /// `TEXMEM_4MB` (5).
    pub const FOUR_MB: Self = Self(5);
    /// `TEXMEM_2MB` (6).
    pub const TWO_MB: Self = Self(6);

    /// The setting value a profile stores.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The stored value.
    pub const fn value(self) -> u32 {
        self.0
    }

    /// The budget in MiB this setting value registers.
    ///
    /// Measured: value 1 registers 12 MiB, 2 registers 10 MiB, 3 registers 8,
    /// 4 registers 6, 5 registers 4, 6 registers 2 and every other value —
    /// `TEXMEM_MAX` included — registers 0.
    pub const fn budget_mib(self) -> u32 {
        match self.0 {
            1 => 12,
            2 => 10,
            3 => 8,
            4 => 6,
            5 => 4,
            6 => 2,
            _ => 0,
        }
    }

    /// The token the enum table gives this value, when it names one.
    pub const fn token(self) -> Option<&'static str> {
        match self.0 {
            0 => Some("TEXMEM_MAX"),
            3 => Some("TEXMEM_8MB"),
            4 => Some("TEXMEM_6MB"),
            5 => Some("TEXMEM_4MB"),
            6 => Some("TEXMEM_2MB"),
            _ => None,
        }
    }

    /// The value `detail.zrd` gives `TextureMemory_SW` on a machine with
    /// `ram_kb` of RAM: `TEXMEM_MAX` from 256000 KB, `TEXMEM_8MB` from
    /// 128000 KB, `TEXMEM_4MB` from 64000 KB and `TEXMEM_2MB` below that.
    /// `TextureMemory_HW` defaults to [`Self::MAX`] on every machine.
    pub const fn software_default_for_ram_kb(ram_kb: u64) -> Self {
        if ram_kb >= 256_000 {
            Self::MAX
        } else if ram_kb >= 128_000 {
            Self::EIGHT_MB
        } else if ram_kb >= 64_000 {
            Self::FOUR_MB
        } else {
            Self::TWO_MB
        }
    }
}

impl fmt::Display for TextureMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.token() {
            Some(token) => write!(f, "{token} ({})", self.0),
            None => write!(f, "unnamed ({})", self.0),
        }
    }
}

/// The video-options panel's texture rows (`VIDEO.SCRIPT` `vp_d_texture`).
///
/// The panel holds three rows. The original stores the selected one as an
/// index, which native callback `2133` reads and writes in the panel's UI copy
/// at `0x648368`, and writes to `TextureMemory_HW` when a hardware device is
/// selected and to `TextureMemory_SW` otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TextureDetailRow {
    /// Row 0, the smallest budget: `TEXMEM_6MB`.
    Low,
    /// Row 1: `TEXMEM_8MB`.
    Middle,
    /// Row 2, the largest: `TEXMEM_MAX`.
    High,
}

impl TextureDetailRow {
    /// Every row, in the order the panel lists them.
    pub const ALL: [Self; 3] = [Self::Low, Self::Middle, Self::High];

    /// The index the original stores for this row.
    pub const fn index(self) -> u32 {
        match self {
            Self::Low => 0,
            Self::Middle => 1,
            Self::High => 2,
        }
    }

    /// The row the panel shows for `setting` when it loads it (`0x418f1a`).
    ///
    /// Values 0, 1 and 2 all show the top row, 3 the middle row and 4, 5 and 6
    /// the bottom row. The mapping covers the whole enum domain of
    /// [`TextureMemory`]; a value outside it — reachable only from a
    /// hand-edited profile — takes the same branch as 4, 5 and 6, which the
    /// evidence does not cover (recorded in the finding).
    pub const fn for_setting(setting: TextureMemory) -> Self {
        match setting.value() {
            0..=2 => Self::High,
            3 => Self::Middle,
            _ => Self::Low,
        }
    }

    /// The setting value the panel writes when the player picks this row
    /// (`0x419392`): the bottom row writes `TEXMEM_6MB`, the middle row
    /// `TEXMEM_8MB` and the top row `TEXMEM_MAX`.
    pub const fn setting(self) -> TextureMemory {
        match self {
            Self::Low => TextureMemory::SIX_MB,
            Self::Middle => TextureMemory::EIGHT_MB,
            Self::High => TextureMemory::MAX,
        }
    }
}

/// What the renderer was doing when a world was loaded.
///
/// Measured at `0x52faf0`/`0x530fe0`: the renderer mode decides whether the
/// budget comes from the device or from the detail setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererMode {
    /// The software renderer: the setting path with `TextureMemory_SW`, and no
    /// r-flag.
    Software,
    /// The hardware renderer. `total_texture_mib` is what
    /// `IDirectDraw::GetAvailableVidMem(DDSCAPS_TEXTURE)` reported as total
    /// texture memory in MiB, and `None` when there was no DirectDraw object
    /// at all — which takes the setting path with `TextureMemory_HW`, still
    /// without the r-flag.
    Hardware {
        /// The device's total texture memory in MiB.
        total_texture_mib: Option<u32>,
    },
}

/// The one texture-archive descriptor a world load registers: the budget its
/// file-name loop walks and whether the `r`-prefixed name comes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureBudget {
    /// The budget in MiB. 0 means the probe starts at the unnumbered name.
    pub mib: u32,
    /// The original's r-flag: the `r`-prefixed candidate is probed before the
    /// plain one.
    pub reduced_prefix_first: bool,
}

/// What a world load knows about itself when it registers its one texture
/// descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldTextureLoad {
    /// Which renderer was configured.
    pub renderer: RendererMode,
    /// The `TextureMemory_HW` setting.
    pub hardware_memory: TextureMemory,
    /// The `TextureMemory_SW` setting.
    pub software_memory: TextureMemory,
}

impl WorldTextureLoad {
    /// A load with both settings at `setting`.
    pub const fn with_setting(renderer: RendererMode, setting: TextureMemory) -> Self {
        Self {
            renderer,
            hardware_memory: setting,
            software_memory: setting,
        }
    }

    /// The load this project's renderer makes: hardware with
    /// [`PROJECT_HARDWARE_TEXTURE_MIB`] MiB of texture memory and the measured
    /// detail defaults.
    pub const fn project_default() -> Self {
        Self {
            renderer: RendererMode::Hardware {
                total_texture_mib: Some(PROJECT_HARDWARE_TEXTURE_MIB),
            },
            hardware_memory: TextureMemory::MAX,
            software_memory: TextureMemory::MAX,
        }
    }
}

/// The texture budget this project registers, in MiB.
///
/// **Designed, not measured.** The original reads the device's total texture
/// memory through `IDirectDraw::GetAvailableVidMem(DDSCAPS_TEXTURE)`
/// (`0x5a0ae0`), which this project's renderer has no equivalent of, so the
/// number is a project choice on top of the measured rule: 16 MiB is at least
/// the largest tier any retail world group ships (`ZBD/C1`'s `rtexture15`), so
/// every world group selects its top tier — what the original selects on a card
/// with 16 MiB or more of texture memory. Recorded as `Designed` in
/// `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`.
pub const PROJECT_HARDWARE_TEXTURE_MIB: u32 = 16;

/// The descriptor one world load registers, from the renderer and the detail
/// setting.
///
/// Measured (`0x52faf0` → `0x52fa50` → `0x530fe0`):
///
/// * hardware with a DirectDraw total: that total in MiB, r-flag set;
/// * hardware without a DirectDraw object: `TextureMemory_HW`'s budget, no
///   r-flag;
/// * software: `TextureMemory_SW`'s budget, no r-flag.
pub const fn texture_budget(load: &WorldTextureLoad) -> TextureBudget {
    match load.renderer {
        RendererMode::Hardware {
            total_texture_mib: Some(total),
        } => TextureBudget {
            mib: total,
            reduced_prefix_first: true,
        },
        RendererMode::Hardware {
            total_texture_mib: None,
        } => TextureBudget {
            mib: load.hardware_memory.budget_mib(),
            reduced_prefix_first: false,
        },
        RendererMode::Software => TextureBudget {
            mib: load.software_memory.budget_mib(),
            reduced_prefix_first: false,
        },
    }
}

/// The key variant the designed mount layout serves a world group or
/// installation file under (F04's `SessionBuilder::mount_installation`).
pub const TEXTURE_ARCHIVE_VARIANT: &str = "default";

/// The unnumbered world archive: `k = 0` of the budget loop, `texture.zbd`.
pub const WORLD_ARCHIVE_FILE: &str = "texture.zbd";

/// The shared image archive the original registers once at app init: its
/// r-flag is always set, so it resolves to `rimage.zbd` in the global `zbd`
/// directory whatever the budget is.
pub const IMAGE_ARCHIVE_FILE: &str = "rimage.zbd";

/// The global directory the texture search falls back to.
pub const GLOBAL_TEXTURE_DIRECTORY: &str = "zbd";

/// One texture search directory: what the original's file probe can see in it,
/// and the key a file found here resolves under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureDirectory {
    label: String,
    namespace: &'static str,
    prefix: String,
    files: Vec<String>,
}

impl TextureDirectory {
    /// A search directory that is neither a world group nor the global
    /// fallback: the key a file found here resolves under is built from
    /// `namespace` and `prefix`.
    pub fn new(
        label: impl Into<String>,
        namespace: &'static str,
        prefix: impl Into<String>,
        files: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            label: label.into(),
            namespace,
            prefix: prefix.into(),
            files: files.into_iter().collect(),
        }
    }

    /// A world group's own texture directory, the original's `zbd\<group>`:
    /// its files resolve as `world/default/<file>`, so two worlds never share
    /// one archive.
    pub fn world(label: impl Into<String>, files: impl IntoIterator<Item = String>) -> Self {
        Self::new(label, WORLD_NAMESPACE, "", files)
    }

    /// The global fallback directory, searched last: its files resolve as
    /// `install/default/zbd/<file>`, because the installation mount spells
    /// paths below its own root.
    pub fn global(files: impl IntoIterator<Item = String>) -> Self {
        Self::new(
            GLOBAL_TEXTURE_DIRECTORY,
            INSTALL_NAMESPACE,
            GLOBAL_TEXTURE_DIRECTORY,
            files,
        )
    }

    /// The directory's own spelling, for diagnostics: `zbd\c1` for a world
    /// group, `zbd` for the global fallback.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Every file name the directory holds.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// The key a file named `file` in this directory resolves under.
    ///
    /// # Errors
    ///
    /// [`AssetKeyError`] when the name is not a usable key path, which cannot
    /// happen for a name this crate generates.
    pub fn key_for(&self, file: &str) -> Result<AssetKey, AssetKeyError> {
        let path = if self.prefix.is_empty() {
            file.to_owned()
        } else {
            format!("{}/{file}", self.prefix)
        };
        AssetKey::from_spelling(self.namespace, &path, TEXTURE_ARCHIVE_VARIANT)
    }
}

/// Where the original's file probe found a name: which texture directory,
/// counted in search order, and the file's own spelling there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundFile {
    directory: usize,
    name: String,
    key: AssetKey,
}

impl FoundFile {
    /// Which search directory held it; 0 is the most recently added one.
    pub const fn directory(&self) -> usize {
        self.directory
    }

    /// The file's name, as that directory spells it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The key this file resolves under in the session.
    pub const fn key(&self) -> &AssetKey {
        &self.key
    }
}

impl fmt::Display for FoundFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in texture directory {}", self.name, self.directory)
    }
}

/// The texture search directories, as the original's "does this file exist"
/// probe (`0x59d170`) sees them.
///
/// `SetTextureDirectory` (`0x52fb40`) adds one directory per call, keeps only
/// the directories that exist and searches the **most recently added first**,
/// falling back to the global `zbd` directory. In retail the list is the
/// world's own directory alone, so a world archive can only come from that
/// directory or from `zbd` — never from another world.
///
/// Only names live here: a caller can list a directory it can see without being
/// able to open the files in it, which is all the probe needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextureFiles {
    directories: Vec<TextureDirectory>,
}

impl TextureFiles {
    /// An empty search list: no candidate file exists anywhere.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the list from directories given in search order — the most
    /// recently added first, the global fallback last.
    pub fn new_with(directories: impl IntoIterator<Item = TextureDirectory>) -> Self {
        Self {
            directories: directories.into_iter().collect(),
        }
    }

    /// A world group's own directory plus the global fallback, which is the
    /// whole list a retail world load searches.
    pub fn world_and_global(
        world_files: impl IntoIterator<Item = String>,
        global_files: impl IntoIterator<Item = String>,
    ) -> Self {
        Self::new_with([
            TextureDirectory::world("zbd/world", world_files),
            TextureDirectory::global(global_files),
        ])
    }

    /// The number of directories the probe searches.
    pub fn len(&self) -> usize {
        self.directories.len()
    }

    /// Whether no directory is searched at all.
    pub fn is_empty(&self) -> bool {
        self.directories.is_empty()
    }

    /// The directory at `index` of the search order.
    pub fn directory(&self, index: usize) -> Option<&TextureDirectory> {
        self.directories.get(index)
    }

    /// Every directory, in search order.
    pub fn directories(&self) -> impl Iterator<Item = &TextureDirectory> {
        self.directories.iter()
    }

    /// Where the probe finds `name`.
    ///
    /// Candidate names are compared against the listing exactly. The original
    /// probes the host file system, whose names it folds without regard to
    /// case; every candidate this crate generates is lowercase, and retail
    /// spells every archive that way, so the comparison decides the same files
    /// here (recorded in the finding).
    ///
    /// A listing entry that is not a usable key spelling is not a candidate:
    /// nothing could resolve it.
    pub fn find(&self, name: &str) -> Option<FoundFile> {
        self.directories
            .iter()
            .enumerate()
            .find_map(|(index, directory)| {
                directory
                    .files()
                    .iter()
                    .any(|listed| listed == name)
                    .then(|| {
                        // The listing is already known to hold the name, so this only
                        // fails for a caller-supplied name no key can spell.
                        FoundFile {
                            directory: index,
                            name: name.to_owned(),
                            key: directory.key_for(name).unwrap_or_else(|error| {
                                panic!("a listed name is a usable key: {error}")
                            }),
                        }
                    })
            })
    }

    /// Whether the probe finds `name`.
    pub fn exists(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// The largest tier number any directory holds, as in `rtexture15.zbd`.
    ///
    /// The original counts the budget down one integer at a time. Everything
    /// above this number is missing by construction, so the selection starts
    /// here: the same file is opened and the same order is kept, without
    /// walking thousands of names a DirectDraw total could ask for.
    pub fn highest_tier(&self) -> Option<u32> {
        self.directories
            .iter()
            .flat_map(TextureDirectory::files)
            .filter_map(|listed| tier_number(listed.as_str()))
            .max()
    }
}

/// The tier number of a numbered archive name, `texture12.zbd` and
/// `rtexture12.zbd` both being 12, and the unnumbered name being `None`.
fn tier_number(name: &str) -> Option<u32> {
    let rest = name
        .strip_prefix("rtexture")
        .or_else(|| name.strip_prefix("texture"))?;
    rest.strip_suffix(".zbd")
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

/// One probed candidate file name, in the order the original probed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveProbe {
    /// The candidate as the original spells it.
    pub file: String,
    /// Where the probe found it, or `None` when it does not exist.
    pub found: Option<FoundFile>,
}

impl ArchiveProbe {
    /// Whether this candidate exists, and so is the archive the load opens.
    pub fn exists(&self) -> bool {
        self.found.is_some()
    }
}

/// The single archive one world load opens, and every candidate it walked to
/// get there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldArchiveChoice {
    budget: TextureBudget,
    probes: Vec<ArchiveProbe>,
    opened: Option<FoundFile>,
}

impl WorldArchiveChoice {
    /// The descriptor the load registered.
    pub const fn budget(&self) -> TextureBudget {
        self.budget
    }

    /// Every candidate the probe walked, in order, up to and including the one
    /// that exists.
    pub fn probes(&self) -> &[ArchiveProbe] {
        &self.probes
    }

    /// The one file the load opens, when a candidate existed.
    pub fn opened(&self) -> Option<&FoundFile> {
        self.opened.as_ref()
    }

    /// The name of the one file the load opens.
    pub fn opened_name(&self) -> Option<&str> {
        self.opened.as_ref().map(FoundFile::name)
    }

    /// Whether a candidate existed at all. When it did not, the original
    /// marks the descriptor failed and the world has no archive textures.
    pub const fn is_open(&self) -> bool {
        self.opened.is_some()
    }
}

/// The one texture archive a world load opens (`0x531500`).
///
/// The measured loop, exactly:
///
/// * budget 0 first tries `rtexture.zbd` — only when the r-flag is set — and
///   then `texture.zbd`; if neither exists the budget becomes 8 and the loop
///   below runs, which probes the unnumbered name a second time;
/// * then, counting down from the budget to 0, each `k` probes
///   `texture<k>.zbd` (or `texture.zbd` at `k = 0`), the `r`-prefixed name
///   first when the r-flag is set;
/// * the first candidate that exists wins. Nothing falls back to another tier
///   for a texture the chosen archive does not hold.
pub fn select_world_archive(files: &TextureFiles, load: &WorldTextureLoad) -> WorldArchiveChoice {
    let budget = texture_budget(load);
    let mut probes = Vec::new();
    let mut opened = None;
    let mut top = budget.mib;

    if top == 0 {
        if budget.reduced_prefix_first {
            opened = probe(files, &mut probes, &format!("r{WORLD_ARCHIVE_FILE}"));
        }
        if opened.is_none() {
            opened = probe(files, &mut probes, WORLD_ARCHIVE_FILE);
        }
        if opened.is_none() {
            top = 8;
        }
    }
    if opened.is_none() {
        // Everything above the listing's highest tier is missing by
        // construction; see `TextureFiles::highest_tier`.
        let top = top.min(files.highest_tier().unwrap_or(0));
        for tier in (0..=top).rev() {
            let candidate = if tier == 0 {
                WORLD_ARCHIVE_FILE.to_owned()
            } else {
                format!("texture{tier}.zbd")
            };
            if budget.reduced_prefix_first {
                opened = probe(files, &mut probes, &format!("r{candidate}"));
            }
            if opened.is_none() {
                opened = probe(files, &mut probes, &candidate);
            }
            if opened.is_some() {
                break;
            }
        }
    }
    WorldArchiveChoice {
        budget,
        probes,
        opened,
    }
}

/// Probes one candidate name and records the step.
fn probe(files: &TextureFiles, probes: &mut Vec<ArchiveProbe>, file: &str) -> Option<FoundFile> {
    let found = files.find(file);
    probes.push(ArchiveProbe {
        file: file.to_owned(),
        found: found.clone(),
    });
    found
}

/// One place the original looks for a texture name, in the order it looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureLookupSource {
    /// The world's one texture archive.
    WorldArchive {
        /// The archive the world load opened.
        file: FoundFile,
    },
    /// The shared image archive, `rimage.zbd`.
    ImageArchive {
        /// Where the file probe found it.
        file: FoundFile,
    },
    /// A loose `<name>.tif` in a texture directory.
    LooseTiff {
        /// Where the file probe found it.
        file: FoundFile,
    },
    /// A loose `<name>.bmp` in a texture directory.
    LooseBmp {
        /// Where the file probe found it.
        file: FoundFile,
    },
}

impl TextureLookupSource {
    /// The file this source would read.
    pub const fn file(&self) -> &FoundFile {
        match self {
            Self::WorldArchive { file }
            | Self::ImageArchive { file }
            | Self::LooseTiff { file }
            | Self::LooseBmp { file } => file,
        }
    }
}

/// The order the original searches for one texture name
/// (`0x531b60` → `0x531900` → `0x531a60` → `0x531930`).
///
/// The world's archive is searched first, then the shared `rimage.zbd`. A name
/// in neither is looked for as a loose `<name>.tif` and then `<name>.bmp` in
/// the texture directories and then `zbd`, behind a flag the image keeps set.
/// Only sources that exist are listed, so the first entry is the original's
/// answer.
///
/// The name is folded to lower case first, because the in-archive search
/// (`0x531930`) folds it, and the folded spelling is the one the loose file
/// names are built from.
///
/// The original also keeps a toggle that starts each search in whichever of
/// the two archive lists it names — the world's first — and flips it when a
/// name is missed, so a miss leaves the lists in the other order. The two
/// namespaces share no name in retail, so no order can change an answer here;
/// the toggle is a last-hit cache, not a lookup rule.
pub fn texture_lookup_order(
    name: &str,
    files: &TextureFiles,
    world_archive: &str,
) -> Vec<TextureLookupSource> {
    let folded = name.to_lowercase();
    let mut sources = Vec::new();
    if let Some(file) = files.find(world_archive) {
        sources.push(TextureLookupSource::WorldArchive { file });
    }
    if let Some(file) = files.find(IMAGE_ARCHIVE_FILE) {
        sources.push(TextureLookupSource::ImageArchive { file });
    }
    if let Some(file) = files.find(&format!("{folded}.tif")) {
        sources.push(TextureLookupSource::LooseTiff { file });
    }
    if let Some(file) = files.find(&format!("{folded}.bmp")) {
        sources.push(TextureLookupSource::LooseBmp { file });
    }
    sources
}

/// Why a world load could not be given a texture archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldTextureError {
    /// No candidate file existed, so the original marks the descriptor failed
    /// and the world has no archive textures at all.
    NoArchive {
        /// Every candidate the probe walked, in order.
        probes: Vec<ArchiveProbe>,
    },
    /// The chosen file's name is not a usable key spelling.
    Key {
        /// The file the choice named.
        file: String,
        /// The refusal, as the key renders it.
        detail: String,
    },
}

impl WorldTextureError {
    /// Stable lowercase identifier.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoArchive { .. } => "no_texture_archive",
            Self::Key { .. } => "asset_key",
        }
    }
}

impl fmt::Display for WorldTextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoArchive { probes } => {
                let walked = probes
                    .iter()
                    .map(|probe| probe.file.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "no texture archive exists; probed {walked}")
            }
            Self::Key { file, detail } => write!(f, "{file} is not a usable key: {detail}"),
        }
    }
}

impl std::error::Error for WorldTextureError {}

impl TextureCatalog {
    /// Opens the one texture archive a world load registers, choosing it by
    /// [`select_world_archive`] instead of by a name the caller guesses.
    ///
    /// `files` is what the original's file probe can see: the world's own
    /// directory first, the global `zbd` directory last. The chosen file is
    /// opened through the key [`FoundFile::key`] carries, which the F04
    /// designed mount layout resolves in the directory it was found in. A
    /// chosen file that does not open as a texture package stays a failed
    /// catalog row ([`Self::failures`]) instead of being dropped.
    ///
    /// # Errors
    ///
    /// [`WorldTextureError::NoArchive`] when no candidate file exists, which
    /// is what the original's failed descriptor means; and
    /// [`WorldTextureError::Key`] when the chosen name cannot be spelled as a
    /// key.
    pub fn open_world(
        session: &ContentSession,
        files: &TextureFiles,
        load: &WorldTextureLoad,
    ) -> Result<(Self, WorldArchiveChoice), WorldTextureError> {
        let choice = select_world_archive(files, load);
        let opened = choice
            .opened()
            .ok_or_else(|| WorldTextureError::NoArchive {
                probes: choice.probes().to_vec(),
            })?
            .clone();
        let key = opened.key().clone();
        Ok((Self::open(session, std::slice::from_ref(&key)), choice))
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

    /// F08-D.02: a resolution is bound to the catalog that made it, not only
    /// to the session. Two catalogs opened from one session over one archive
    /// share the generation, so the sibling must be refused by the catalog
    /// serial alone; a session that read neither catalog is refused first,
    /// with its own code; and each catalog still serves what it resolved.
    #[test]
    fn accept_f08_d_02_sibling_catalog_of_the_same_session_is_refused() {
        let tree = Tree::two_worlds();
        let session = world_session(&tree.0, "ZBD/c1");
        let key = world_key("texture.zbd");
        let first = TextureCatalog::open(&session, std::slice::from_ref(&key));
        let second = TextureCatalog::open(&session, std::slice::from_ref(&key));
        assert_eq!(
            first.generation(),
            second.generation(),
            "the siblings share the session generation, which is why it cannot decide"
        );
        assert_ne!(first.serial(), second.serial(), "two catalogs, two serials");

        let resolved = first
            .resolve(&session, &texture_ref("sky"))
            .expect("sky resolves");
        assert_eq!(resolved.serial(), first.serial());

        // The sibling holds the same archive, at the same span, with the same
        // entry: only the serial tells it this resolution is not its own.
        let refused = second
            .prepare_upload(&session, &resolved)
            .expect_err("a sibling catalog does not answer for it");
        assert_eq!(refused.code(), "not_from_this_catalog");
        match refused {
            UploadError::Resolve(TextureResolveError::NotFromThisCatalog { id }) => {
                assert_eq!(*id, *resolved.id());
            }
            other => panic!("expected a refusal naming the texture, got {other:?}"),
        }
        assert_eq!(
            second
                .decode(&session, &resolved)
                .expect_err("a sibling does not decode it either")
                .code(),
            "not_from_this_catalog"
        );

        // A session that read neither catalog is refused before the catalog
        // identity, and with its own code.
        let foreign = world_session(&tree.0, "ZBD/c2");
        assert_eq!(
            first
                .prepare_upload(&foreign, &resolved)
                .expect_err("foreign session")
                .code(),
            "foreign_session"
        );

        // Each catalog still uploads what it resolved itself.
        let first_upload = first
            .prepare_upload(&session, &resolved)
            .expect("the owning catalog uploads");
        assert_eq!(first_upload.id(), resolved.id());
        assert_eq!(words(&first_upload), SKY_C1);
        let by_entry = second
            .resolve_entry(&session, &key, 1)
            .expect("the sibling resolves the entry itself");
        assert_eq!(by_entry.serial(), second.serial());
        let second_upload = second
            .prepare_upload(&session, &by_entry)
            .expect("the sibling uploads what it resolved");
        assert_eq!(words(&second_upload), SKY_C1);
    }

    // --- the texture-archive selection rule (task #352) ---------------------

    const WHITE: u16 = 0xFFFF;

    /// The file names a world group probes: the unnumbered archive plus the
    /// 2/4/6/8 tiers and one top tier, as every retail world group ships.
    fn tier_names(top: u32) -> Vec<String> {
        let mut names: Vec<String> = [top, 8, 6, 4, 2]
            .into_iter()
            .map(|tier| format!("rtexture{tier}.zbd"))
            .collect();
        names.push(WORLD_ARCHIVE_FILE.to_owned());
        names
    }

    /// Every candidate name the choice walked, in order.
    fn probed(choice: &WorldArchiveChoice) -> Vec<String> {
        choice
            .probes()
            .iter()
            .map(|probe| probe.file.clone())
            .collect()
    }

    /// The names a directory holds, sorted; what the original's file probe
    /// sees in it.
    fn directory_listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("{} is listed: {error}", dir.display()))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("directory entry of {}: {error}", dir.display()))
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    /// The search list a world group load sees: its own directory, then the
    /// global `zbd` directory.
    fn search_list(root: &Path, group: &str) -> TextureFiles {
        TextureFiles::new_with([
            TextureDirectory::world(group, directory_listing(&root.join(group))),
            TextureDirectory::global(directory_listing(&root.join(GLOBAL_TEXTURE_DIRECTORY))),
        ])
    }

    /// Budget 0 — the measured `TEXMEM_MAX` path — and a large hardware budget
    /// walk exactly the candidates the original walks, the `r`-prefixed name
    /// first where the r-flag is set.
    #[test]
    fn accept_f08_c_selection_budget_zero_and_large_budgets_walk_the_measured_loop() {
        let files =
            TextureFiles::world_and_global(tier_names(15), vec![IMAGE_ARCHIVE_FILE.to_owned()]);

        // Budget 0 without the r-flag: the probe starts at the unnumbered name
        // and never tries `rtexture.zbd`.
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(RendererMode::Software, TextureMemory::MAX),
        );
        assert_eq!(
            choice.budget(),
            TextureBudget {
                mib: 0,
                reduced_prefix_first: false
            }
        );
        assert_eq!(probed(&choice), vec![WORLD_ARCHIVE_FILE.to_owned()]);
        assert_eq!(choice.opened_name(), Some(WORLD_ARCHIVE_FILE));
        assert!(choice.is_open());

        // A 16 MiB device takes the largest tier the group ships. The walk
        // starts there: every candidate above the largest tier the search
        // directories hold is missing by construction.
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(16),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(
            choice.budget(),
            TextureBudget {
                mib: 16,
                reduced_prefix_first: true
            }
        );
        assert_eq!(probed(&choice), vec!["rtexture15.zbd".to_owned()]);
        assert_eq!(choice.opened_name(), Some("rtexture15.zbd"));
        assert_eq!(
            choice
                .opened()
                .expect("a candidate exists")
                .key()
                .to_string(),
            "world/default/rtexture15.zbd"
        );
        assert_eq!(
            choice.probes()[0].found.as_ref().map(FoundFile::directory),
            Some(0)
        );

        // A 16 MiB device on a group whose tiers reach 16 counts down from the
        // budget, probing the `r`-prefixed name before the plain one at each
        // tier and stopping at the first that exists.
        let wide = TextureFiles::world_and_global(
            vec![
                WORLD_ARCHIVE_FILE.to_owned(),
                "rtexture2.zbd".to_owned(),
                "rtexture4.zbd".to_owned(),
                "rtexture6.zbd".to_owned(),
                "rtexture8.zbd".to_owned(),
                "texture16.zbd".to_owned(),
            ],
            Vec::new(),
        );
        let choice = select_world_archive(
            &wide,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(16),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(
            probed(&choice),
            vec!["rtexture16.zbd".to_owned(), "texture16.zbd".to_owned()],
            "`rtexture16.zbd` is probed first and `texture16.zbd` opens it"
        );
        assert_eq!(choice.opened_name(), Some("texture16.zbd"));

        // The same budget on a group whose tiers stop at 8 walks down to 8.
        let choice = select_world_archive(
            &wide,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(8),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(
            probed(&choice),
            vec!["rtexture8.zbd".to_owned()],
            "the budget caps the walk even when higher tiers exist"
        );
        assert_eq!(choice.opened_name(), Some("rtexture8.zbd"));

        // With nothing numbered to find, the walk ends at `k = 0` and probes
        // the unnumbered `r`-prefixed name before the plain one.
        let bare = TextureFiles::world_and_global(vec![WORLD_ARCHIVE_FILE.to_owned()], Vec::new());
        let choice = select_world_archive(
            &bare,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(8),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(
            probed(&choice),
            vec!["rtexture.zbd".to_owned(), WORLD_ARCHIVE_FILE.to_owned(),]
        );
        assert_eq!(choice.opened_name(), Some(WORLD_ARCHIVE_FILE));

        // Budget 0 with the r-flag set tries the unnumbered `r` name first and
        // takes it from the global directory when only it exists there.
        let global = TextureFiles::new_with([
            TextureDirectory::world("zbd/c1", vec![WORLD_ARCHIVE_FILE.to_owned()]),
            TextureDirectory::global(vec!["rtexture.zbd".to_owned()]),
        ]);
        let choice = select_world_archive(
            &global,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(0),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(probed(&choice), vec!["rtexture.zbd".to_owned()]);
        let opened = choice.opened().expect("the global name exists");
        assert_eq!(opened.directory(), 1, "found in the fallback directory");
        assert_eq!(opened.key().to_string(), "install/default/zbd/rtexture.zbd");
    }

    /// A world group that ships fewer tiers than the budget walks down to the
    /// largest one it has, then to the unnumbered archive; a world group with
    /// no candidate at all opens nothing and says so.
    #[test]
    fn accept_f08_c_selection_missing_tiers_walk_down_to_the_unnumbered_name() {
        let files = TextureFiles::world_and_global(
            vec![
                WORLD_ARCHIVE_FILE.to_owned(),
                "rtexture6.zbd".to_owned(),
                "rtexture2.zbd".to_owned(),
            ],
            Vec::new(),
        );

        // 16 MiB: the walk stops at the largest tier the group ships.
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(16),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(choice.opened_name(), Some("rtexture6.zbd"));

        // A budget below every tier it ships walks down to the largest tier at
        // or below the budget.
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(5),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(choice.opened_name(), Some("rtexture2.zbd"));
        assert_eq!(
            probed(&choice),
            vec![
                "rtexture5.zbd".to_owned(),
                "texture5.zbd".to_owned(),
                "rtexture4.zbd".to_owned(),
                "texture4.zbd".to_owned(),
                "rtexture3.zbd".to_owned(),
                "texture3.zbd".to_owned(),
                "rtexture2.zbd".to_owned(),
            ]
        );

        // Below every tier, including below 2, the walk ends at the
        // unnumbered names.
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(1),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(choice.opened_name(), Some(WORLD_ARCHIVE_FILE));
        assert_eq!(
            probed(&choice),
            vec![
                "rtexture1.zbd".to_owned(),
                "texture1.zbd".to_owned(),
                "rtexture.zbd".to_owned(),
                WORLD_ARCHIVE_FILE.to_owned(),
            ],
            "the walk counts down one tier at a time, `r` first, and ends at k = 0"
        );

        // No candidate at all: nothing is opened, and the catalog refuses the
        // world with the walked names rather than reporting an empty archive.
        let empty = TextureFiles::world_and_global(vec!["gamez.zbd".to_owned()], Vec::new());
        let choice = select_world_archive(
            &empty,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(16),
                },
                TextureMemory::MAX,
            ),
        );
        assert!(!choice.is_open());
        assert_eq!(choice.opened_name(), None);
        assert!(choice.probes().iter().all(|probe| !probe.exists()));
        assert_eq!(
            probed(&choice),
            vec!["rtexture.zbd".to_owned(), WORLD_ARCHIVE_FILE.to_owned(),]
        );

        let tree = Tree::two_worlds();
        let session = world_session(&tree.0, "ZBD/c1");
        let files = search_list(&tree.0, "ZBD/c1");
        // The fixture's world one has only the unnumbered archive, which the
        // unignored tests above already walk to; an empty listing opens nothing.
        assert_eq!(
            select_world_archive(&files, &WorldTextureLoad::project_default()).opened_name(),
            Some(WORLD_ARCHIVE_FILE)
        );
        let error = TextureCatalog::open_world(
            &session,
            &TextureFiles::world_and_global(Vec::new(), Vec::new()),
            &WorldTextureLoad::project_default(),
        )
        .expect_err("no candidate file");
        assert_eq!(error.code(), "no_texture_archive");
        assert!(
            error.to_string().contains("rtexture.zbd")
                && error.to_string().contains(WORLD_ARCHIVE_FILE),
            "the refusal names what it probed: {error}"
        );
    }

    /// The setting path registers the detail setting's budget and no r-flag:
    /// `TextureMemory_SW` for the software renderer, `TextureMemory_HW` when
    /// there is no DirectDraw object.
    #[test]
    fn accept_f08_c_selection_the_setting_path_registers_the_detail_setting() {
        // The measured value → budget table, including the two values the enum
        // table leaves unnamed and a value outside its domain.
        for (value, budget) in [
            (0u32, 0u32),
            (1, 12),
            (2, 10),
            (3, 8),
            (4, 6),
            (5, 4),
            (6, 2),
            (7, 0),
            (99, 0),
        ] {
            assert_eq!(
                TextureMemory::new(value).budget_mib(),
                budget,
                "setting value {value}"
            );
            assert_eq!(
                texture_budget(&WorldTextureLoad::with_setting(
                    RendererMode::Software,
                    TextureMemory::new(value)
                )),
                TextureBudget {
                    mib: budget,
                    reduced_prefix_first: false
                },
                "the software renderer never sets the r-flag"
            );
            assert_eq!(
                texture_budget(&WorldTextureLoad::with_setting(
                    RendererMode::Hardware {
                        total_texture_mib: None
                    },
                    TextureMemory::new(value)
                )),
                TextureBudget {
                    mib: budget,
                    reduced_prefix_first: false
                },
                "hardware without DirectDraw takes the setting path"
            );
        }

        // `detail.zrd`'s defaults: `TEXMEM_MAX` for the hardware setting and
        // the RAM-dependent software one.
        assert_eq!(
            TextureMemory::software_default_for_ram_kb(262_144),
            TextureMemory::MAX
        );
        assert_eq!(
            TextureMemory::software_default_for_ram_kb(128_000),
            TextureMemory::EIGHT_MB
        );
        assert_eq!(
            TextureMemory::software_default_for_ram_kb(64_000),
            TextureMemory::FOUR_MB
        );
        assert_eq!(
            TextureMemory::software_default_for_ram_kb(63_999),
            TextureMemory::TWO_MB
        );
        assert_eq!(
            WorldTextureLoad::project_default().hardware_memory,
            TextureMemory::MAX
        );

        // Which setting each renderer reads.
        let load = WorldTextureLoad {
            renderer: RendererMode::Software,
            hardware_memory: TextureMemory::EIGHT_MB,
            software_memory: TextureMemory::TWO_MB,
        };
        assert_eq!(
            texture_budget(&load).mib,
            2,
            "software reads TextureMemory_SW"
        );
        let load = WorldTextureLoad {
            renderer: RendererMode::Hardware {
                total_texture_mib: None,
            },
            ..load
        };
        assert_eq!(
            texture_budget(&load).mib,
            8,
            "hardware reads TextureMemory_HW"
        );
        let load = WorldTextureLoad {
            renderer: RendererMode::Hardware {
                total_texture_mib: Some(3),
            },
            ..load
        };
        assert_eq!(
            texture_budget(&load),
            TextureBudget {
                mib: 3,
                reduced_prefix_first: true
            },
            "a DirectDraw total outranks both settings"
        );

        // On a retail-shaped group the setting path lands on the unnumbered
        // archive without ever probing an `r`-prefixed name: no plain numbered
        // tier ships.
        let files = TextureFiles::world_and_global(tier_names(8), Vec::new());
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(RendererMode::Software, TextureMemory::EIGHT_MB),
        );
        assert_eq!(choice.opened_name(), Some(WORLD_ARCHIVE_FILE));
        assert_eq!(
            probed(&choice).first().map(String::as_str),
            Some("texture8.zbd")
        );
        assert!(
            probed(&choice).iter().all(|name| !name.starts_with('r')),
            "no `r`-prefixed candidate without the r-flag: {:?}",
            probed(&choice)
        );
        assert_eq!(probed(&choice).len(), 9, "k = 8 down to k = 0");

        // With the r-flag set the same budget takes the tier, because only the
        // `r` spelling ships: this is the pair that separates the two paths.
        let choice = select_world_archive(
            &files,
            &WorldTextureLoad::with_setting(
                RendererMode::Hardware {
                    total_texture_mib: Some(8),
                },
                TextureMemory::MAX,
            ),
        );
        assert_eq!(choice.opened_name(), Some("rtexture8.zbd"));

        // Both spellings present: the r-flag decides which one opens.
        let both = TextureFiles::world_and_global(
            vec![
                WORLD_ARCHIVE_FILE.to_owned(),
                "rtexture12.zbd".to_owned(),
                "texture12.zbd".to_owned(),
            ],
            Vec::new(),
        );
        assert_eq!(
            select_world_archive(
                &both,
                &WorldTextureLoad::with_setting(
                    RendererMode::Hardware {
                        total_texture_mib: Some(12)
                    },
                    TextureMemory::MAX
                )
            )
            .opened_name(),
            Some("rtexture12.zbd")
        );
        assert_eq!(
            select_world_archive(
                &both,
                &WorldTextureLoad::with_setting(RendererMode::Software, TextureMemory::new(1))
            )
            .opened_name(),
            Some("texture12.zbd")
        );
    }

    /// The video-options dropdown: three rows, the measured setting each
    /// writes, and the archive every row leaves a retail world group on.
    #[test]
    fn accept_f08_c_selection_the_video_dropdown_rows_map_to_the_measured_settings() {
        // The rows, their stored index and the setting they write.
        assert_eq!(
            TextureDetailRow::ALL
                .iter()
                .map(|row| (row.index(), row.setting()))
                .collect::<Vec<_>>(),
            vec![
                (0, TextureMemory::SIX_MB),
                (1, TextureMemory::EIGHT_MB),
                (2, TextureMemory::MAX),
            ]
        );
        // The panel's load direction.
        for (value, row) in [
            (0u32, TextureDetailRow::High),
            (1, TextureDetailRow::High),
            (2, TextureDetailRow::High),
            (3, TextureDetailRow::Middle),
            (4, TextureDetailRow::Low),
            (5, TextureDetailRow::Low),
            (6, TextureDetailRow::Low),
        ] {
            assert_eq!(
                TextureDetailRow::for_setting(TextureMemory::new(value)),
                row,
                "setting value {value}"
            );
        }
        // Picking a row and reloading it shows the same row.
        for row in TextureDetailRow::ALL {
            assert_eq!(
                TextureDetailRow::for_setting(row.setting()),
                row,
                "{row:?} survives a round trip"
            );
        }

        // What the dropdown does to the archive on a retail-shaped group: with
        // the software renderer, or without a DirectDraw object, every row and
        // the hardware setting alike land on the unnumbered archive, because
        // only the `r` spelling ships and those paths do not set the r-flag.
        let files = TextureFiles::world_and_global(tier_names(15), Vec::new());
        for renderer in [
            RendererMode::Software,
            RendererMode::Hardware {
                total_texture_mib: None,
            },
        ] {
            for row in TextureDetailRow::ALL {
                let setting = row.setting();
                let choice = select_world_archive(
                    &files,
                    &WorldTextureLoad::with_setting(renderer, setting),
                );
                assert_eq!(
                    choice.opened_name(),
                    Some(WORLD_ARCHIVE_FILE),
                    "{renderer:?} with row {row:?} ({setting})"
                );
            }
        }
        // On a hardware DirectDraw device the dropdown cannot change the
        // archive at all: the device's total decides it.
        for row in TextureDetailRow::ALL {
            let choice = select_world_archive(
                &files,
                &WorldTextureLoad {
                    renderer: RendererMode::Hardware {
                        total_texture_mib: Some(PROJECT_HARDWARE_TEXTURE_MIB),
                    },
                    ..WorldTextureLoad::with_setting(RendererMode::Software, row.setting())
                },
            );
            assert_eq!(
                choice.opened_name(),
                Some("rtexture15.zbd"),
                "row {row:?} does not change a hardware device's archive"
            );
        }
    }

    /// The order one texture name is looked up in: the world's archive, then
    /// `rimage.zbd`, then a loose `.tif` and `.bmp`.
    #[test]
    fn accept_f08_c_selection_the_lookup_order_is_world_archive_image_archive_then_loose_files() {
        let files = TextureFiles::new_with([
            TextureDirectory::world(
                "zbd/c1",
                vec![
                    WORLD_ARCHIVE_FILE.to_owned(),
                    "rtexture15.zbd".to_owned(),
                    "sky.tif".to_owned(),
                    "hud.bmp".to_owned(),
                ],
            ),
            TextureDirectory::global(vec![IMAGE_ARCHIVE_FILE.to_owned()]),
        ]);
        let sources = texture_lookup_order("sky", &files, WORLD_ARCHIVE_FILE);
        assert_eq!(
            sources
                .iter()
                .map(|source| source.file().name().to_owned())
                .collect::<Vec<_>>(),
            vec![
                WORLD_ARCHIVE_FILE.to_owned(),
                IMAGE_ARCHIVE_FILE.to_owned(),
                "sky.tif".to_owned(),
            ],
            "the world's archive, the image archive, then the loose TIFF"
        );
        assert!(matches!(
            sources[0],
            TextureLookupSource::WorldArchive { .. }
        ));
        assert!(matches!(
            sources[1],
            TextureLookupSource::ImageArchive { .. }
        ));
        assert!(matches!(sources[2], TextureLookupSource::LooseTiff { .. }));
        assert_eq!(
            sources[1].file().key().to_string(),
            "install/default/zbd/rimage.zbd"
        );
        assert_eq!(
            sources[0].file().key().to_string(),
            "world/default/texture.zbd"
        );

        // A mixed-case request is folded, so it finds the lower-case name in
        // the archive search and in the loose file names.
        assert_eq!(
            texture_lookup_order("SKY", &files, WORLD_ARCHIVE_FILE),
            sources
        );

        // A `.bmp` with no `.tif` beside it is still searched, after the two
        // archives.
        assert_eq!(
            texture_lookup_order("hud", &files, WORLD_ARCHIVE_FILE)
                .last()
                .map(|source| (
                    source.file().name().to_owned(),
                    matches!(source, TextureLookupSource::LooseBmp { .. })
                )),
            Some(("hud.bmp".to_owned(), true))
        );

        // The tier that was not selected is not a lookup source, and a name in
        // no source at all yields the image archive alone rather than a guess.
        assert_eq!(
            texture_lookup_order("ground", &files, WORLD_ARCHIVE_FILE).len(),
            2,
            "only the two archives hold or can hold a world name"
        );
        // The order follows the archive the load selected rather than a fixed
        // name: naming another archive makes it the first source.
        let other = texture_lookup_order("ground", &files, "rtexture15.zbd");
        assert!(matches!(
            &other[0],
            TextureLookupSource::WorldArchive { file } if file.name() == "rtexture15.zbd"
        ));
        assert_eq!(other[1].file().name(), IMAGE_ARCHIVE_FILE);
        // An archive that does not exist is no source at all.
        let absent = texture_lookup_order("ground", &files, "rtexture99.zbd");
        assert_eq!(absent.len(), 1);
        assert!(matches!(
            &absent[0],
            TextureLookupSource::ImageArchive { .. }
        ));
    }

    /// The rule reaches the catalog: `open_world` opens the archive the
    /// measured rule selects, resolves a name from it, and refuses a world
    /// group whose tier does not exist.
    #[test]
    fn accept_f08_c_selection_open_world_catalogs_the_archive_the_rule_selects() {
        // The world's top tier holds its own `sky`, so the texels say which
        // archive served the name.
        const TIERED_SKY: [u16; 6] = [CYAN, MAGENTA, RED, GREEN, BLUE, YELLOW];
        let tree = Tree::new();
        tree.write(
            "ZBD/c1/texture.zbd",
            &package(&[Tex::direct("sky", OPAQUE, 3, 2, &SKY_C1)]),
        );
        tree.write(
            "ZBD/c1/rtexture15.zbd",
            &package(&[Tex::direct("sky", OPAQUE, 3, 2, &TIERED_SKY)]),
        );
        tree.write(
            "ZBD/c2/texture.zbd",
            &package(&[Tex::direct("sky", OPAQUE, 3, 2, &SKY_C2)]),
        );
        tree.write(
            "ZBD/rimage.zbd",
            &package(&[Tex::direct("hud_mark", OPAQUE, 1, 1, &[WHITE])]),
        );

        // The project's own load picks world one's top tier, and the texture
        // comes out of that tier.
        let session = world_session(&tree.0, "ZBD/c1");
        let (catalog, choice) = TextureCatalog::open_world(
            &session,
            &search_list(&tree.0, "ZBD/c1"),
            &WorldTextureLoad::project_default(),
        )
        .expect("the selected archive opens");
        assert_eq!(choice.opened_name(), Some("rtexture15.zbd"));
        assert_eq!(
            choice.budget(),
            TextureBudget {
                mib: PROJECT_HARDWARE_TEXTURE_MIB,
                reduced_prefix_first: true
            }
        );
        assert_eq!(catalog.failures().count(), 0);
        let archive = catalog.archives().next().expect("one archive");
        assert_eq!(archive.path().as_str(), "ZBD/c1/rtexture15.zbd");
        assert_eq!(archive.key().to_string(), "world/default/rtexture15.zbd");

        let sky = catalog
            .resolve(&session, &TextureRef::new(archive.key().clone(), "sky"))
            .expect("sky resolves");
        assert_eq!(sky.id().archive.as_str(), "ZBD/c1/rtexture15.zbd");
        assert_eq!(
            words(&catalog.prepare_upload(&session, &sky).expect("uploads")),
            TIERED_SKY.to_vec()
        );

        // The same key and name under the software renderer come out of the
        // other world's unnumbered archive.
        let session = world_session(&tree.0, "ZBD/c2");
        let (catalog, choice) = TextureCatalog::open_world(
            &session,
            &search_list(&tree.0, "ZBD/c2"),
            &WorldTextureLoad::with_setting(RendererMode::Software, TextureMemory::MAX),
        )
        .expect("the selected archive opens");
        assert_eq!(choice.opened_name(), Some(WORLD_ARCHIVE_FILE));
        let archive = catalog.archives().next().expect("one archive");
        assert_eq!(archive.path().as_str(), "ZBD/c2/texture.zbd");
        let sky = catalog
            .resolve(&session, &TextureRef::new(archive.key().clone(), "sky"))
            .expect("sky resolves");
        assert_eq!(
            words(&catalog.prepare_upload(&session, &sky).expect("uploads")),
            SKY_C2.to_vec()
        );

        // The shared image archive is reachable through the global directory,
        // and it is a texture package like the world's own.
        let image = TextureArchive::open(
            &session,
            &AssetKey::from_spelling(INSTALL_NAMESPACE, "zbd/rimage.zbd", TEXTURE_ARCHIVE_VARIANT)
                .expect("the image archive key is valid"),
        )
        .expect("the shared image archive opens");
        assert_eq!(image.path().as_str(), "ZBD/rimage.zbd");
        assert_eq!(image.ids().count(), 1);

        // A world group whose selected tier exists but is not a texture
        // package stays a failed catalog row, not a silent empty archive.
        let tree = Tree::two_worlds();
        tree.write("ZBD/c1/rtexture15.zbd", b"not a texture package");
        let session = world_session(&tree.0, "ZBD/c1");
        let (catalog, choice) = TextureCatalog::open_world(
            &session,
            &search_list(&tree.0, "ZBD/c1"),
            &WorldTextureLoad::project_default(),
        )
        .expect("the name exists, so a catalog is returned");
        assert_eq!(choice.opened_name(), Some("rtexture15.zbd"));
        assert_eq!(catalog.archives().count(), 0);
        assert_eq!(catalog.failures().count(), 1);
        assert_eq!(
            catalog
                .resolve(
                    &session,
                    &TextureRef::new(archive_key("rtexture15.zbd"), "sky")
                )
                .expect_err("the archive failed")
                .code(),
            "archive_failed"
        );
    }

    /// The key a world's archive file resolves under.
    fn archive_key(file: &str) -> AssetKey {
        AssetKey::from_spelling(WORLD_NAMESPACE, file, TEXTURE_ARCHIVE_VARIANT)
            .expect("a texture archive key is valid")
    }

    /// Retail: every world group selects the archive the measured rule names,
    /// and the selected archive is the one the catalog opens.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f08_c_selection_retail_world_groups_select_the_measured_archive() {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must point at the original installation for this test");
        let root = PathBuf::from(dir);
        assert!(
            root.is_dir(),
            "CS_GAME_DIR {} is not a directory",
            root.display()
        );

        /// The measured tiers of one world group: the largest tier it ships
        /// and what a hardware budget of 16, 12, 8, 7 and 1 MiB selects, plus
        /// what the software renderer selects.
        struct RetailTiers {
            group: &'static str,
            top: u32,
            at_16: &'static str,
            at_12: &'static str,
            at_8: &'static str,
            at_7: &'static str,
            at_1: &'static str,
        }
        const TABLE: &[RetailTiers] = &[
            RetailTiers {
                group: "ZBD/C1",
                top: 15,
                at_16: "rtexture15.zbd",
                at_12: "rtexture8.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C1B",
                top: 11,
                at_16: "rtexture11.zbd",
                at_12: "rtexture11.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C1C",
                top: 10,
                at_16: "rtexture10.zbd",
                at_12: "rtexture10.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C2",
                top: 14,
                at_16: "rtexture14.zbd",
                at_12: "rtexture8.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C2B",
                top: 9,
                at_16: "rtexture9.zbd",
                at_12: "rtexture9.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C3",
                top: 12,
                at_16: "rtexture12.zbd",
                at_12: "rtexture12.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C4",
                top: 14,
                at_16: "rtexture14.zbd",
                at_12: "rtexture8.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
            RetailTiers {
                group: "ZBD/C5",
                top: 14,
                at_16: "rtexture14.zbd",
                at_12: "rtexture8.zbd",
                at_8: "rtexture8.zbd",
                at_7: "rtexture6.zbd",
                at_1: "texture.zbd",
            },
        ];

        let global = search_list(&root, "ZBD").directories().count();
        assert_eq!(global, 2, "the search list is the world directory and zbd");
        for expected in TABLE {
            let files = search_list(&root, expected.group);
            assert_eq!(
                files.highest_tier(),
                Some(expected.top),
                "{} ships tiers up to {}",
                expected.group,
                expected.top
            );
            let world_listing = files.directory(0).expect("the world directory");
            let texture_family: Vec<&String> = world_listing
                .files()
                .iter()
                .filter(|name| {
                    name.as_str() == WORLD_ARCHIVE_FILE || tier_number(name.as_str()).is_some()
                })
                .collect();
            assert_eq!(
                texture_family.len(),
                6,
                "{} stores the unnumbered archive and five tiers: {texture_family:?}",
                expected.group
            );

            for (total, want) in [
                (Some(16u32), expected.at_16),
                (Some(12), expected.at_12),
                (Some(8), expected.at_8),
                (Some(7), expected.at_7),
                (Some(1), expected.at_1),
                // No DirectDraw object: the setting path, which on retail
                // lands on the unnumbered archive.
                (None, WORLD_ARCHIVE_FILE),
            ] {
                let choice = select_world_archive(
                    &files,
                    &WorldTextureLoad::with_setting(
                        RendererMode::Hardware {
                            total_texture_mib: total,
                        },
                        TextureMemory::MAX,
                    ),
                );
                assert_eq!(
                    choice.opened_name(),
                    Some(want),
                    "{} with a hardware total of {total:?} MiB probed {:?}",
                    expected.group,
                    probed(&choice)
                );
                assert_eq!(
                    choice.budget().reduced_prefix_first,
                    total.is_some(),
                    "only a DirectDraw total sets the r-flag"
                );
            }

            // The software renderer, at every setting the panel can write and
            // at both unnamed values, always opens the palettized archive.
            for setting in [
                TextureMemory::MAX,
                TextureMemory::new(1),
                TextureMemory::new(2),
                TextureMemory::EIGHT_MB,
                TextureMemory::SIX_MB,
                TextureMemory::FOUR_MB,
                TextureMemory::TWO_MB,
            ] {
                let choice = select_world_archive(
                    &files,
                    &WorldTextureLoad::with_setting(RendererMode::Software, setting),
                );
                assert_eq!(
                    choice.opened_name(),
                    Some(WORLD_ARCHIVE_FILE),
                    "{} with the software renderer at {setting}",
                    expected.group
                );
            }
        }

        // The rule reaches the catalog on the real installation: world one's
        // project default opens its top tier, which holds the whole name set,
        // and the shared image archive opens from the global directory.
        let found = install::discover(&root).expect("installation is discovered");
        let session = session_of(&root, &found, "ZBD/C1");
        let (catalog, choice) = TextureCatalog::open_world(
            &session,
            &search_list(&root, "ZBD/C1"),
            &WorldTextureLoad::project_default(),
        )
        .expect("world one's selected archive opens");
        assert_eq!(choice.opened_name(), Some("rtexture15.zbd"));
        assert_eq!(catalog.failures().count(), 0);
        let archive = catalog.archives().next().expect("one archive");
        assert_eq!(archive.path().as_str(), "ZBD/C1/rtexture15.zbd");
        assert_eq!(archive.ids().count(), 881, "C1's top tier name count");

        let image = TextureArchive::open(
            &session,
            &AssetKey::from_spelling(INSTALL_NAMESPACE, "zbd/rimage.zbd", TEXTURE_ARCHIVE_VARIANT)
                .expect("the image archive key is valid"),
        )
        .expect("the shared image archive opens");
        assert_eq!(image.path().as_str(), "ZBD/rimage.zbd");
        assert_eq!(image.ids().count(), 254, "rimage.zbd name count");

        // The two namespaces are disjoint, so the lookup order cannot change an
        // answer: every world archive name is absent from `rimage.zbd`.
        let world_names: std::collections::BTreeSet<String> =
            archive.ids().map(|id| id.name.to_lowercase()).collect();
        let shared = image
            .ids()
            .filter(|id| world_names.contains(&id.name.to_lowercase()))
            .count();
        assert_eq!(shared, 0, "no name is in both archives");
        eprintln!(
            "F08-C selection: 8 world groups, top tiers 9-15, software renderer and every dropdown row select {}",
            WORLD_ARCHIVE_FILE
        );
    }
}
