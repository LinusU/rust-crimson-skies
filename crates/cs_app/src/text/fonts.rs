//! Original font loading: the declared fonts, their measured metrics and the
//! private load/retry transaction menus, HUD and subtitles share (F51-C).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-C`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F51-A declared [`FontFace`] — a content id, a family, a provenance and a
//! declared glyph coverage — and made the two unacceptable sources impossible to
//! hold: an operating-system font and a redistributed proprietary game font are
//! refused by `FontFace::try_new`. F51-B declared [`TextMetrics`], the measured
//! seam a layout reads. What neither stage owns is the *load*: a face and its
//! metrics becoming one usable font, and that load failing and being retried
//! without leaving a half-built set behind. That is what this module adds.
//!
//! # The format is still unknown, and that is not hidden
//!
//! The original font format is **unmeasured**; no parser lives here and none is
//! guessed. [`FontMeasurer`] is the declared seam where a real decode lands
//! (F51-D, once the format is known), and a measurer that cannot read a face
//! reports it rather than returning invented numbers. [`FontSet::load`] therefore
//! fails as a whole when one face cannot be measured, so a session never starts
//! with a subset of the fonts its content asked for. An original installation
//! font is loaded privately through [`FontFace`]'s provenance; the release never
//! redistributes it, which [`LoadedFont::is_distributable`] keeps honest.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::localization::{FontCatalog, FontFace};
use cs_types::content::ContentId;

use super::metrics::TextMetrics;

/// Why a font could not be loaded or reloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontLoadError {
    /// The catalog declared no face at all, so a session built on it could lay
    /// nothing out.
    Empty,
    /// A face's metrics could not be measured. The original font format is
    /// unmeasured at this stage, so a caller-supplied [`FontMeasurer`] reports
    /// this instead of a guessed layout.
    Unmeasured {
        /// The family of the face that could not be measured.
        family: String,
        /// Why it could not be measured.
        reason: String,
    },
    /// A font id is already in the set.
    Duplicate {
        /// The duplicated id.
        id: ContentId,
    },
    /// A font id the caller named is not in the set.
    Absent {
        /// The missing id.
        id: ContentId,
    },
}

impl fmt::Display for FontLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => {
                f.write_str("the font catalog declares no face, so nothing could be laid out")
            }
            Self::Unmeasured { family, reason } => {
                write!(f, "font {family:?} could not be measured: {reason}")
            }
            Self::Duplicate { id } => write!(f, "font {id} is loaded more than once"),
            Self::Absent { id } => write!(f, "font {id} is not loaded"),
        }
    }
}

impl std::error::Error for FontLoadError {}

/// Measures a declared font face's layout metrics.
///
/// This is the declared seam F51-D fills once the original font format is
/// measured. It is deliberately not implemented in this crate: the only honest
/// production answer today is "unmeasured", and a caller that *has* measured a
/// face supplies the numbers. Coverage travels with the metrics, so a face's
/// declared [`GlyphCoverage`](cs_content::localization::GlyphCoverage) — not a
/// guess here — is what the layout counts missing glyphs against.
pub trait FontMeasurer {
    /// The metrics of `face`, or why it could not be measured.
    ///
    /// # Errors
    ///
    /// A free-text reason: the format is unmeasured, a table is unsupported, the
    /// bytes are corrupt. It becomes [`FontLoadError::Unmeasured`].
    fn measure(&self, face: &FontFace) -> Result<TextMetrics, String>;
}

/// One font loaded privately: its declared face and its measured metrics.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedFont {
    face: FontFace,
    metrics: TextMetrics,
}

impl LoadedFont {
    /// The font's content id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        self.face.id()
    }

    /// The declared face: family, provenance and declared coverage.
    #[must_use]
    pub fn face(&self) -> &FontFace {
        &self.face
    }

    /// The measured metrics a layout reads.
    #[must_use]
    pub fn metrics(&self) -> &TextMetrics {
        &self.metrics
    }

    /// The declared family name.
    #[must_use]
    pub fn family(&self) -> &str {
        self.face.family()
    }

    /// Whether the release may distribute this font.
    ///
    /// An original private font never may — it is loaded from the owner's
    /// installation and stays private — and a licensed fallback may only when
    /// its permission was verified. Both answers come from the face's own
    /// provenance, so a packaging step cannot ship a font by accident.
    #[must_use]
    pub fn is_distributable(&self) -> bool {
        self.face.provenance().is_distributable()
    }
}

/// The fonts a text session can lay text out with, keyed by font id.
///
/// Rows are kept in id order, so a report over the set is deterministic and does
/// not depend on load order.
#[derive(Clone, Debug, Default)]
pub struct FontSet {
    fonts: BTreeMap<ContentId, LoadedFont>,
}

impl FontSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fonts: BTreeMap::new(),
        }
    }

    /// Loads every face a catalog declares, measuring each through `measurer`.
    ///
    /// The whole set is built before it is returned, so a face that cannot be
    /// measured fails the load and leaves the caller with nothing rather than a
    /// half-populated set a session would silently start with. A catalog with no
    /// face is [`FontLoadError::Empty`]: a set that can lay nothing out is not a
    /// usable font set.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Empty`] or [`FontLoadError::Unmeasured`].
    pub fn load(catalog: &FontCatalog, measurer: &dyn FontMeasurer) -> Result<Self, FontLoadError> {
        let mut set = Self::new();
        for face in catalog.faces() {
            set.load_face(face, measurer)?;
        }
        if set.fonts.is_empty() {
            return Err(FontLoadError::Empty);
        }
        Ok(set)
    }

    /// Measures one face and inserts (or replaces) it.
    fn load_face(
        &mut self,
        face: &FontFace,
        measurer: &dyn FontMeasurer,
    ) -> Result<(), FontLoadError> {
        let metrics = measurer
            .measure(face)
            .map_err(|reason| FontLoadError::Unmeasured {
                family: face.family().to_owned(),
                reason,
            })?;
        self.fonts.insert(
            face.id().clone(),
            LoadedFont {
                face: face.clone(),
                metrics,
            },
        );
        Ok(())
    }

    /// Inserts an already-measured font, refusing an id that is already loaded.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Duplicate`].
    pub fn insert(&mut self, face: FontFace, metrics: TextMetrics) -> Result<(), FontLoadError> {
        let id = face.id().clone();
        if self.fonts.contains_key(&id) {
            return Err(FontLoadError::Duplicate { id });
        }
        self.fonts.insert(id, LoadedFont { face, metrics });
        Ok(())
    }

    /// Retries one face's load, replacing whatever a previous attempt left.
    ///
    /// A failed load is not stored, so the retry starts clean: a measurer that
    /// succeeds the second time replaces nothing and a face that was never
    /// loaded is simply loaded.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Unmeasured`].
    pub fn reload(
        &mut self,
        face: &FontFace,
        measurer: &dyn FontMeasurer,
    ) -> Result<(), FontLoadError> {
        self.load_face(face, measurer)
    }

    /// The font with `id`, if the set holds one.
    #[must_use]
    pub fn get(&self, id: &ContentId) -> Option<&LoadedFont> {
        self.fonts.get(id)
    }

    /// The metrics of `id`, if the set holds it.
    #[must_use]
    pub fn metrics(&self, id: &ContentId) -> Option<&TextMetrics> {
        self.fonts.get(id).map(LoadedFont::metrics)
    }

    /// Whether `id` is loaded.
    #[must_use]
    pub fn contains(&self, id: &ContentId) -> bool {
        self.fonts.contains_key(id)
    }

    /// How many fonts the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    /// Whether the set holds no font.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// The loaded fonts, in id order.
    pub fn fonts(&self) -> impl Iterator<Item = &LoadedFont> {
        self.fonts.values()
    }

    /// The loaded fonts the release may distribute, in id order.
    pub fn distributable(&self) -> impl Iterator<Item = &LoadedFont> {
        self.fonts.values().filter(|font| font.is_distributable())
    }
}
