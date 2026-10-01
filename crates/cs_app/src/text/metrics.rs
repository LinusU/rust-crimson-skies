//! The text measurement input: advance widths, line metrics and glyph coverage
//! at a pixel size (F51-A).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-A`. This is the typed seam where F51-B's font parsing lands:
//! the layout in [`crate::text::layout`] never opens a font file, it only asks
//! a [`TextMetrics`] how wide a character is and whether the font has it.
//!
//! The synthetic monospace stand-in ([`synthetic_monospace`]) is a **designed**
//! development fixture with a fixed advance, an explicit line height and the
//! ASCII printable range as its declared coverage. It is not a measurement of
//! the original game, and it exists so AC01 can be exercised headless without
//! shipping a font.

use std::collections::BTreeMap;

use cs_content::localization::GlyphCoverage;

/// The measured shape of one font at one pixel size.
///
/// Every value a layout needs is a constructor argument, so a caller cannot
/// hand the layout a half-measured font: a NaN or a non-positive size is
/// refused, and a [`GlyphCoverage`] that has not been read yet is the empty set
/// (which correctly reports *every* character as missing rather than pretending
/// the font can draw them).
#[derive(Clone, Debug, PartialEq)]
pub struct TextMetrics {
    pixel_size: f32,
    advances: BTreeMap<char, f32>,
    default_advance: f32,
    line_height: f32,
    ascent: f32,
    coverage: GlyphCoverage,
}

/// Why a [`TextMetrics`] set was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextMetricsError {
    /// The pixel size was NaN, infinite or not positive.
    InvalidPixelSize {
        /// The rejected size.
        pixel_size: f32,
    },
    /// An advance width was NaN, infinite or negative.
    InvalidAdvance {
        /// The character the advance belongs to, if one was named.
        ch: Option<char>,
        /// The rejected advance.
        advance: f32,
    },
    /// The line height or the ascent was NaN, infinite or not positive.
    InvalidVerticalMetric {
        /// The rejected value.
        value: f32,
    },
    /// The ascent was above the line height, which cannot be laid out.
    AscentAboveLineHeight {
        /// The declared ascent.
        ascent: f32,
        /// The declared line height.
        line_height: f32,
    },
}

impl std::fmt::Display for TextMetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPixelSize { pixel_size } => {
                write!(
                    f,
                    "text pixel size {pixel_size} must be finite and positive"
                )
            }
            Self::InvalidAdvance { ch, advance } => write!(
                f,
                "advance width {advance} for {} must be finite and non-negative",
                ch.map_or_else(|| "the fallback glyph".to_owned(), |ch| ch.to_string())
            ),
            Self::InvalidVerticalMetric { value } => {
                write!(
                    f,
                    "vertical text metric {value} must be finite and positive"
                )
            }
            Self::AscentAboveLineHeight {
                ascent,
                line_height,
            } => write!(f, "ascent {ascent} is above the line height {line_height}"),
        }
    }
}

impl std::error::Error for TextMetricsError {}

impl TextMetrics {
    /// Validates a measured font.
    ///
    /// `advances` holds the per-character advance widths the font actually
    /// declares; `default_advance` is used for any character it does not, which
    /// is how a proportional font is approximated until F51-B's real tables
    /// arrive. `coverage` is the declared glyph coverage — pass
    /// [`GlyphCoverage::new`] while it is unmeasured, so the gap is counted
    /// rather than hidden.
    ///
    /// # Errors
    ///
    /// [`TextMetricsError`] for a non-finite or non-positive size, advance or
    /// vertical metric, and for an ascent above the line height.
    pub fn try_new(
        pixel_size: f32,
        advances: BTreeMap<char, f32>,
        default_advance: f32,
        line_height: f32,
        ascent: f32,
        coverage: GlyphCoverage,
    ) -> Result<Self, TextMetricsError> {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        if !positive(pixel_size) {
            return Err(TextMetricsError::InvalidPixelSize { pixel_size });
        }
        for (ch, advance) in &advances {
            if !advance.is_finite() || *advance < 0.0 {
                return Err(TextMetricsError::InvalidAdvance {
                    ch: Some(*ch),
                    advance: *advance,
                });
            }
        }
        if !default_advance.is_finite() || default_advance < 0.0 {
            return Err(TextMetricsError::InvalidAdvance {
                ch: None,
                advance: default_advance,
            });
        }
        if !positive(line_height) {
            return Err(TextMetricsError::InvalidVerticalMetric { value: line_height });
        }
        if !positive(ascent) {
            return Err(TextMetricsError::InvalidVerticalMetric { value: ascent });
        }
        if ascent > line_height {
            return Err(TextMetricsError::AscentAboveLineHeight {
                ascent,
                line_height,
            });
        }
        Ok(Self {
            pixel_size,
            advances,
            default_advance,
            line_height,
            ascent,
            coverage,
        })
    }

    /// The pixel size these metrics were measured at.
    #[must_use]
    pub fn pixel_size(&self) -> f32 {
        self.pixel_size
    }

    /// The distance one line occupies, in pixels.
    #[must_use]
    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// The distance from a line's top to its baseline, in pixels.
    #[must_use]
    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// The declared glyph coverage.
    #[must_use]
    pub fn coverage(&self) -> &GlyphCoverage {
        &self.coverage
    }

    /// Whether the font declares the glyph for `ch`.
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        self.coverage.covers(ch)
    }

    /// The advance width of `ch` in pixels.
    ///
    /// A character the font does not declare still advances — it is drawn as a
    /// visible missing-glyph box — so a missing glyph changes the wrap points
    /// instead of silently collapsing the text.
    #[must_use]
    pub fn advance(&self, ch: char) -> f32 {
        self.advances
            .get(&ch)
            .copied()
            .unwrap_or(self.default_advance)
    }

    /// The advance width of `text` in pixels.
    #[must_use]
    pub fn measure(&self, text: &str) -> f32 {
        text.chars().map(|ch| self.advance(ch)).sum()
    }

    /// These metrics scaled by `factor`, for a UI scale change.
    ///
    /// Coverage is carried over unchanged: scaling does not add glyphs, so a
    /// scale change can never turn a missing glyph into a covered one.
    ///
    /// # Errors
    ///
    /// [`TextMetricsError`] when the factor produces a non-finite or
    /// non-positive size or vertical metric.
    pub fn scaled(&self, factor: f32) -> Result<Self, TextMetricsError> {
        let advances = self
            .advances
            .iter()
            .map(|(ch, advance)| (*ch, advance * factor))
            .collect();
        Self::try_new(
            self.pixel_size * factor,
            advances,
            self.default_advance * factor,
            self.line_height * factor,
            self.ascent * factor,
            self.coverage.clone(),
        )
    }
}

/// The declared monospace development stand-in for a measured font.
///
/// A fixed advance of `0.6 * size`, a line height of `1.2 * size`, an ascent of
/// `0.9 * size` and the ASCII printable range as coverage. It is a **designed**
/// fixture so the layout can be exercised headless; it is not a measurement of
/// the original game's font and carries no original metrics whatsoever.
///
/// # Panics
///
/// Never: the constants satisfy every [`TextMetrics::try_new`] rule at any
/// positive finite size.
#[must_use]
pub fn synthetic_monospace(pixel_size: f32) -> TextMetrics {
    let coverage = GlyphCoverage::from_chars((0x20u8..0x7Fu8).map(char::from));
    TextMetrics::try_new(
        pixel_size,
        BTreeMap::new(),
        pixel_size * 0.6,
        pixel_size * 1.2,
        pixel_size * 0.9,
        coverage,
    )
    .expect("the declared monospace metrics are always valid")
}
