//! The canonical-to-Bevy conversion boundary records (F15-A).
//!
//! Spec `specs/F15-asynchronous-asset-loading-and-private-cache.md`,
//! non-negotiable behavior 1: "Only cs_app converts canonical assets into
//! Bevy assets." The *only* is structural: `cs_content`, `cs_formats`,
//! `cs_assets` and `cs_types` carry no Bevy dependency at all
//! (`docs/01-ARCHITECTURE.md`), so no lower crate can produce a Bevy
//! handle — conversion code has nowhere else to live. What this module
//! adds is the typed boundary around it, so that F15-B/C converters and
//! their consumers share one record shape:
//!
//! * [`CanonicalAsset`] is the input: a stable [`ContentId`], the
//!   [`CacheKey`] the converted form is stored under and the canonical
//!   [`CanonicalPayload`] — decoded bytes plus their digest.
//! * [`ConvertedAsset`] is the output record every converter produces:
//!   generic over the Bevy-side value it wraps, it keeps the content id,
//!   the [`LoadIdentity`] of the load it was made for and the digest of
//!   the cache key it was produced under, so [`ConvertedAsset::
//!   verify_fresh`] can refuse an entry the cache has since invalidated —
//!   the boundary-side half of "a partially written or superseded entry is
//!   never served".
//!
//! The converters themselves arrive with F15-B/F15-C and the per-format
//! adapters (F08+); this stage fixes the records they must produce.

use std::fmt;

use cs_assets::cache::CacheKey;
use cs_assets::install::sha256;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use crate::loading::LoadIdentity;

/// Decoded canonical bytes and their digest, the payload half of a
/// [`CanonicalAsset`].
///
/// The digest is computed at construction, so what the converter hashes
/// and what a later integrity check hashes are the same bytes — a payload
/// can disagree with a previously recorded digest but never misreport its
/// own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPayload {
    kind: ContentKind,
    bytes: Vec<u8>,
    sha256: ContentHash,
}

impl CanonicalPayload {
    /// Wraps decoded bytes of `kind`, computing their digest.
    pub fn new(kind: ContentKind, bytes: Vec<u8>) -> Self {
        Self {
            kind,
            sha256: sha256(&bytes),
            bytes,
        }
    }

    /// The content kind the bytes decode as.
    pub fn kind(&self) -> ContentKind {
        self.kind
    }

    /// The canonical bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// SHA-256 of the bytes, computed at construction.
    pub fn sha256(&self) -> ContentHash {
        self.sha256
    }
}

/// The typed input of the conversion boundary: what a converter is allowed
/// to see.
///
/// `cache_key` is the full identity the converted form will be stored
/// under — installation, source spans, decoder/IR version and options —
/// so a converter can never produce an asset without also producing the
/// key that invalidates it.
#[derive(Clone, Debug)]
pub struct CanonicalAsset {
    /// The stable content id.
    pub content: ContentId,
    /// The cache identity of the converted form.
    pub cache_key: CacheKey,
    /// The canonical bytes and their digest.
    pub payload: CanonicalPayload,
}

/// Why a converted asset refused to stand in for a requested derivation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversionError {
    /// The asset was produced under a different cache key than the one
    /// being asked for — the entry it was built from is stale (a changed
    /// source, decoder, IR or option set). It is rebuilt, not reused.
    StaleCacheKey {
        /// The key digest the asset was produced under.
        produced_under: ContentHash,
        /// The key digest the consumer needs.
        expected: ContentHash,
    },
    /// The converter refused the canonical payload: its decoder rejected
    /// the bytes, or the composition budget did not admit the output.
    /// `detail` carries the converter's own reason, and the load records
    /// it as a `conversion` failure with `Abort` recovery — a payload the
    /// converter cannot produce is not retried as a transient fault.
    Failed {
        /// Why, in the converter's own words.
        detail: String,
    },
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleCacheKey {
                produced_under,
                expected,
            } => write!(
                f,
                "the asset was converted under key {produced_under}, not the \
                 requested {expected}: the cached derivation is stale"
            ),
            Self::Failed { detail } => write!(f, "the conversion failed: {detail}"),
        }
    }
}

impl std::error::Error for ConversionError {}

/// The output record of the conversion boundary: a Bevy-side value plus
/// the stamps that make it accountable.
///
/// `T` is whatever the converter produces — a `bevy::asset::Handle`, a
/// spawned component set, a decoded buffer for a system to upload. The
/// record around it is what F15's non-negotiables need from every
/// converter alike: which content it is, which load it belongs to (so a
/// stale-session product is detectable like a stale-session read) and
/// which cache identity it was produced under (so a cache invalidation
/// invalidates the product too).
#[derive(Clone, Debug)]
pub struct ConvertedAsset<T> {
    content: ContentId,
    load: LoadIdentity,
    produced_under: ContentHash,
    output: T,
}

impl<T> ConvertedAsset<T> {
    /// Stamps a produced `output` with its content id, the load it was
    /// converted for and the cache key it was produced under.
    pub fn new(
        content: ContentId,
        load: LoadIdentity,
        produced_under: &CacheKey,
        output: T,
    ) -> Self {
        Self {
            content,
            load,
            produced_under: produced_under.digest(),
            output,
        }
    }

    /// The stable content id.
    pub fn content(&self) -> &ContentId {
        &self.content
    }

    /// The load this product belongs to.
    pub fn load(&self) -> LoadIdentity {
        self.load
    }

    /// The digest of the cache key this product was produced under.
    pub fn produced_under(&self) -> ContentHash {
        self.produced_under
    }

    /// The produced Bevy-side value.
    pub fn output(&self) -> &T {
        &self.output
    }

    /// Consumes the record, returning the produced value.
    pub fn into_output(self) -> T {
        self.output
    }

    /// Whether this product still matches the derivation `key` asks for.
    ///
    /// # Errors
    ///
    /// [`ConversionError::StaleCacheKey`] when the product was built under
    /// a different cache identity.
    pub fn verify_fresh(&self, key: &CacheKey) -> Result<(), ConversionError> {
        if self.produced_under == key.digest() {
            Ok(())
        } else {
            Err(ConversionError::StaleCacheKey {
                produced_under: self.produced_under,
                expected: key.digest(),
            })
        }
    }
}
