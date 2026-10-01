//! Measuring an installation's locales from its own files (F51-LOCALE-SET).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`
//! (deliverable, AC03 and AC04); `specs/F12-text-configuration-strings-and-pe-resources.md`
//! AC04 ("a localized installation preserves stable ids while changing display
//! text"). Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F51-A left the supported-locale set and the resource-language map
//! **caller-declared**, because the original release's locale list was
//! unmeasured, and F51-D audited the owner's installation against a declared set
//! of one locale named by the test. This module is the measurement that replaces
//! that declaration, and it does so in two passes over the original files:
//!
//! * [`measure_string_image_languages`] reads the **localization surface** — the
//!   F12 [`StringRow`]s of the routed string images — and records the
//!   third-level PE resource language ids those images actually carry, each with
//!   the [`SourceSpan`] that proves it. That table is what
//!   [`cs_content::localization::MeasuredLocales::from_table`] turns into the
//!   declaration, so the declared locale set is a function of the files rather
//!   than of a test author's choice.
//! * [`measure_installation_languages`] is the **corroborating census**: every
//!   regular file the production discovery finds under the installation root is
//!   read through the production PE reader, and each image's resource-tree
//!   language ids are recorded. This is what turns "the three string images
//!   carry one language" into "no file in this installation carries any other
//!   resource language", which is the strongest statement one single-language
//!   installation can support.
//!
//! # What is measured, and what is not
//!
//! The measurements above are the *installation's* languages, not the release's
//! supported locales, and the two must not be confused: a release localized into
//! several languages ships one language's strings per installation. Whether a
//! localized installation keeps the id numbering is a second-installation
//! question, and this module does not answer it — it supplies the two catalogs
//! that [`cs_content::localization::measure_id_stability`] compares, and with
//! one installation that comparison honestly reports
//! [`cs_content::localization::IdStability::SingleLocale`].
//!
//! Nothing here writes to the installation, and nothing returns a byte or a
//! character of original text: the records carry paths, spans, digests, counts
//! and the numeric language ids the files recorded.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use cs_assets::install::{self, DiscoveryError};
use cs_content::config::StringRow;
use cs_content::localization::ResourceLanguageTable;
use cs_types::asset_id::SourceSpan;
use cs_types::evidence::ContentHash;

/// The resource language ids of every string image the caller routed, measured.
///
/// Each image contributes **one occurrence per language id it carries**, so a
/// second image carrying the same id is corroborating evidence rather than a
/// duplicate, and an image that carries a language no other image does shows up
/// as its own occurrence. The span is the container's, so a report can name the
/// file and the installation digest the id came from.
#[must_use]
pub fn measure_string_image_languages(
    images: &[StringImageMeasurement<'_>],
) -> ResourceLanguageTable {
    let mut table = ResourceLanguageTable::new();
    for image in images {
        for (language, rows) in string_image_languages(image.rows) {
            table.observe(image.span.clone(), language, rows);
        }
    }
    table
}

/// One routed string image handed to [`measure_string_image_languages`]: the
/// container's [`SourceSpan`] and the F12 rows read out of it.
pub struct StringImageMeasurement<'a> {
    /// The container the rows came from, with the installation digest.
    pub span: &'a SourceSpan,
    /// The F12 rows, each carrying its own language id.
    pub rows: &'a [StringRow],
}

/// The distinct resource language ids in one image's rows, with each id's row
/// count, in ascending id order.
///
/// The count is of *rows*, not of decoded strings: the question is which
/// languages the image was built with, which is true even of a row whose code
/// units did not decode.
#[must_use]
pub fn string_image_languages(rows: &[StringRow]) -> Vec<(u32, usize)> {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for row in rows {
        *counts.entry(row.language).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

/// The resource language ids one PE image's resource tree carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageLanguages {
    /// The installation-relative spelling.
    pub path: String,
    /// The installation digest the bytes were read under.
    pub install: ContentHash,
    /// The image's length in bytes.
    pub bytes: u64,
    /// The distinct third-level resource language ids, in ascending order.
    pub languages: Vec<u32>,
    /// How many resource leaves the tree holds.
    pub leaves: usize,
    /// How many `RT_STRING` blocks the tree holds.
    pub string_blocks: usize,
}

impl ImageLanguages {
    /// Whether the image carries exactly the one language id given.
    #[must_use]
    pub fn carries_only(&self, language: u32) -> bool {
        self.languages.as_slice() == [language]
    }
}

/// What measuring one file's resource languages found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageMeasure {
    /// A PE image with a resource directory: its measured language ids.
    Resources(ImageLanguages),
    /// A PE image that declares no resource directory, or one that declares zero
    /// bytes. The original ships a helper DLL whose data directory is present
    /// but empty, and the resource walker rightly refuses a zero-byte
    /// directory, so this is a measured absence rather than a failure.
    NoResources {
        /// The installation-relative spelling.
        path: String,
        /// The image's length in bytes.
        bytes: u64,
    },
    /// Not a PE image at all: a game archive, an audio bank, a video, a bitmap,
    /// an empty marker file, or a 16-bit DOS/Windows binary that begins with
    /// `MZ` and is not a PE image.
    NotPe {
        /// The installation-relative spelling.
        path: String,
        /// The file's length in bytes.
        bytes: u64,
    },
}

/// One inventoried file the census classified without measuring languages: a PE
/// image with no resource directory, or a file that is not a PE image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLessImage {
    /// The installation-relative spelling.
    pub path: String,
    /// The file's length in bytes.
    pub bytes: u64,
}

/// The installation-wide PE resource language census.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallationLanguages {
    /// The installation digest every record was measured under.
    pub install: ContentHash,
    /// How many regular files the production discovery walked.
    pub files: usize,
    /// The PE images that declare a resource directory, in path order.
    pub images: Vec<ImageLanguages>,
    /// The PE images that declare no resource directory (or an empty one), in
    /// path order.
    pub without_resources: Vec<ResourceLessImage>,
    /// The files that are not PE images at all, with their lengths, in path
    /// order.
    pub not_pe: Vec<ResourceLessImage>,
}

impl InstallationLanguages {
    /// The distinct resource language ids across every measured image, in
    /// ascending order.
    #[must_use]
    pub fn languages(&self) -> Vec<u32> {
        let mut languages: Vec<u32> = self
            .images
            .iter()
            .flat_map(|image| image.languages.iter().copied())
            .collect();
        languages.sort_unstable();
        languages.dedup();
        languages
    }

    /// The census of one image, if it was a PE image with a resource tree.
    #[must_use]
    pub fn image(&self, path: &str) -> Option<&ImageLanguages> {
        self.images.iter().find(|image| image.path == path)
    }
}

/// Why an installation measurement failed.
#[derive(Debug)]
pub enum LocaleMeasureError {
    /// The production discovery could not inventory the installation, so
    /// nothing could be measured under a known digest.
    Discovery(DiscoveryError),
    /// A file in the manifest could not be read.
    Read {
        /// The installation-relative spelling.
        path: String,
        /// The I/O error, verbatim.
        message: String,
    },
    /// A file that claimed to be a PE image could not be parsed.
    Layout {
        /// The installation-relative spelling.
        path: String,
        /// The production reader's own message, verbatim.
        message: String,
    },
}

impl fmt::Display for LocaleMeasureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "installation discovery failed: {error}"),
            Self::Read { path, message } => write!(f, "read {path}: {message}"),
            Self::Layout { path, message } => write!(f, "PE layout of {path}: {message}"),
        }
    }
}

impl std::error::Error for LocaleMeasureError {}

/// Measures every PE image of the installation under `root`.
///
/// The walk is the production discovery ([`install::discover`]), so the file set
/// and the digest are the same ones the rest of the project measures; each file
/// is then read once and handed to the production PE reader. A file that is not a
/// PE image is recorded as such rather than skipped silently, and a file that
/// *is* one but cannot be parsed fails the run by name — a census that quietly
/// dropped the image it could not read would not be a census.
pub fn measure_installation_languages(
    root: &Path,
) -> Result<InstallationLanguages, LocaleMeasureError> {
    let discovery = install::discover(root).map_err(LocaleMeasureError::Discovery)?;
    let install = install::fingerprint(&discovery.manifest);
    let mut census = InstallationLanguages {
        install,
        files: discovery.manifest.files.len(),
        images: Vec::new(),
        without_resources: Vec::new(),
        not_pe: Vec::new(),
    };

    for record in &discovery.manifest.files {
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = discovery
            .manifest
            .host_root
            .join(record.relative_spelling.as_str());
        let bytes = std::fs::read(&path).map_err(|error| LocaleMeasureError::Read {
            path: spelling.clone(),
            message: error.to_string(),
        })?;
        match measure_image_languages(&spelling, install, &bytes)? {
            ImageMeasure::Resources(image) => census.images.push(image),
            ImageMeasure::NoResources { path, bytes } => {
                census
                    .without_resources
                    .push(ResourceLessImage { path, bytes });
            }
            ImageMeasure::NotPe { path, bytes } => {
                census.not_pe.push(ResourceLessImage { path, bytes });
            }
        }
    }
    Ok(census)
}

/// The resource language ids of one file, or why it has none to measure.
///
/// A file whose bytes are not a PE image is not an error: the original ships
/// game archives, audio banks and video alongside its executables, and the
/// census needs to say "not a PE image" rather than fail on them — including the
/// empty marker file, which is too short to hold a header at all, and the 16-bit
/// DOS/Windows binaries, which begin with `MZ` and are not PE images. A file that
/// *is* a PE image but cannot be parsed is an error, because a header this
/// project cannot read is a gap in the measurement rather than an absence of
/// languages.
pub fn measure_image_languages(
    path: &str,
    install: ContentHash,
    bytes: &[u8],
) -> Result<ImageMeasure, LocaleMeasureError> {
    let length = bytes.len() as u64;
    // A file too short to hold even the DOS signature is not a PE image; the
    // original ships an empty marker file, and reading it must not fail a census.
    if bytes.len() < DOS_SIGNATURE.len() {
        return Ok(ImageMeasure::NotPe {
            path: path.to_owned(),
            bytes: length,
        });
    }
    let mut context = cs_formats::ParseContext::with_defaults(path);
    let layout = match cs_formats::read_pe_layout(&mut context, bytes) {
        Ok(layout) => layout,
        Err(error) if is_not_a_pe_image(&error) => {
            return Ok(ImageMeasure::NotPe {
                path: path.to_owned(),
                bytes: length,
            });
        }
        Err(error) => {
            return Err(LocaleMeasureError::Layout {
                path: path.to_owned(),
                message: error.to_string(),
            });
        }
    };
    // No resource data directory, or one that declares zero bytes, is a PE image
    // that carries no resource tree.
    match layout.resource_directory() {
        None => {
            return Ok(ImageMeasure::NoResources {
                path: path.to_owned(),
                bytes: length,
            });
        }
        Some(directory) if directory.size == 0 => {
            return Ok(ImageMeasure::NoResources {
                path: path.to_owned(),
                bytes: length,
            });
        }
        Some(_) => {}
    }
    let resources = cs_formats::read_pe_resources(&mut context, bytes).map_err(|message| {
        LocaleMeasureError::Layout {
            path: path.to_owned(),
            message: message.to_string(),
        }
    })?;

    // The language is the third-level resource key, which F12 records as the raw
    // id the image stored. The tree's depth is the format's business, not this
    // module's, so the id is taken from the same depth the surveyed images use.
    let mut languages: Vec<u32> = Vec::new();
    for leaf in resources.leaves() {
        if let Some(language) = leaf.key(2).and_then(|key| key.id()) {
            languages.push(language);
        }
    }
    languages.sort_unstable();
    languages.dedup();

    Ok(ImageMeasure::Resources(ImageLanguages {
        path: path.to_owned(),
        install,
        bytes: length,
        languages,
        leaves: resources.leaves().len(),
        string_blocks: resources.strings().len(),
    }))
}

/// The DOS signature every PE image starts with.
const DOS_SIGNATURE: &[u8; 2] = b"MZ";

/// Whether the production reader's error is its verdict that these bytes are not
/// a PE image.
///
/// Two refusals are about the file's *kind* rather than about this project's
/// ability to read it: the DOS signature is absent, or it is present and the NT
/// signature is not — the original ships 16-bit DOS/Windows binaries
/// (`clcd16.dll`) that begin with `MZ` and are not PE images at all. Every other
/// refusal (a truncated image, an offset that leaves a table, a directory cycle)
/// is a gap in the measurement, so the census fails the run by name instead of
/// quietly counting an unreadable image as a language-free one.
fn is_not_a_pe_image(error: &cs_formats::pe_resources::PeError) -> bool {
    matches!(
        error,
        cs_formats::pe_resources::PeError::Malformed { field, .. }
            if field == "pe.dos.magic" || field == "pe.signature"
    )
}
