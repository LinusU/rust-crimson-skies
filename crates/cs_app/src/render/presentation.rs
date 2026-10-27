//! The live accessibility session as a render resource, and the colour filter
//! applied to the drawn frame (F52-W2).
//!
//! Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`.
//! [`LivePresentation`] is the one place a run keeps its
//! [`SettingsSession`]; the camera, HUD and audio presenters read it each frame
//! instead of holding a copy, so a change the player applies is what the next
//! frame draws and mixes. [`filter_frame`] is the colour filter over an RGBA
//! framebuffer. The channel weights are **designed** (F52-A: no original option
//! is asserted); see `docs/findings/2026-10-27-f52-w2-presentation-output.md`.

use std::fmt;

use bevy::prelude::Resource;
use cs_content::settings::{ColourFilter, Presentation};

use crate::accessibility::session::SettingsSession;

/// Resource: the run's one live settings session.
#[derive(Resource, Debug)]
pub struct LivePresentation(pub SettingsSession);

impl LivePresentation {
    /// The presentation settings as they are right now.
    #[must_use]
    pub const fn presentation(&self) -> &Presentation {
        self.0.presentation()
    }
}

/// Why a frame could not be filtered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterError {
    /// The buffer is not whole RGBA pixels.
    NotRgba {
        /// The buffer length in bytes.
        len: usize,
    },
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRgba { len } => write!(f, "frame of {len} bytes is not whole RGBA pixels"),
        }
    }
}

impl std::error::Error for FilterError {}

/// The row-major 3x3 matrix a filter multiplies an `[r, g, b]` colour by.
///
/// `None` for [`ColourFilter::Off`]: the frame is left untouched, not
/// multiplied by an identity.
#[must_use]
pub const fn filter_matrix(filter: ColourFilter) -> Option<[[f32; 3]; 3]> {
    match filter {
        ColourFilter::Off => None,
        ColourFilter::Protanopia => Some([
            [0.567, 0.433, 0.0],
            [0.558, 0.442, 0.0],
            [0.0, 0.242, 0.758],
        ]),
        ColourFilter::Deuteranopia => Some([[0.625, 0.375, 0.0], [0.7, 0.3, 0.0], [0.0, 0.3, 0.7]]),
        ColourFilter::Tritanopia => {
            Some([[0.95, 0.05, 0.0], [0.0, 0.433, 0.567], [0.0, 0.475, 0.525]])
        }
        ColourFilter::Monochrome => Some([
            [0.2126, 0.7152, 0.0722],
            [0.2126, 0.7152, 0.0722],
            [0.2126, 0.7152, 0.0722],
        ]),
    }
}

/// Applies `filter` to an RGBA8 frame in place; alpha is never touched.
///
/// # Errors
///
/// [`FilterError::NotRgba`] when `rgba` is not a whole number of pixels; the
/// buffer is left unchanged.
pub fn filter_frame(filter: ColourFilter, rgba: &mut [u8]) -> Result<(), FilterError> {
    if !rgba.len().is_multiple_of(4) {
        return Err(FilterError::NotRgba { len: rgba.len() });
    }
    let Some(matrix) = filter_matrix(filter) else {
        return Ok(());
    };
    for pixel in rgba.as_chunks_mut::<4>().0.iter_mut() {
        let source = [
            f32::from(pixel[0]),
            f32::from(pixel[1]),
            f32::from(pixel[2]),
        ];
        for (channel, row) in matrix.iter().enumerate() {
            let value = row[0] * source[0] + row[1] * source[1] + row[2] * source[2];
            // Rounded and clamped into the byte range, so the cast is exact.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                pixel[channel] = value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

/// Filters a frame under the live session's colour filter, read now.
///
/// # Errors
///
/// As [`filter_frame`].
pub fn present_frame(live: &LivePresentation, rgba: &mut [u8]) -> Result<(), FilterError> {
    filter_frame(live.presentation().colour_filter, rgba)
}
