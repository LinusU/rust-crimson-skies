//! Triangle-strip decoding with explicit parity.
//!
//! A strip of `n` indices describes `n - 2` triangles. Triangle `k` uses the
//! indices at strip positions `k`, `k + 1` and `k + 2`; every odd `k` swaps
//! its first two corners so all triangles keep the winding of the first one
//! (non-negotiable #2 of `specs/F10-gamez-mesh-topology-and-material-
//! records.md`). A degenerate triangle (two equal indices) is still a strip
//! step: it is returned, marked, and it advances the parity. Removing it
//! first and counting parity over the remaining triangles flips every later
//! triangle.
//!
//! Which winding is *front-facing* for Crimson Skies content is not
//! established. This module only guarantees that every triangle of a strip
//! has the winding of the strip's first triangle as stored.

use std::fmt;

/// Fewest indices that describe one triangle.
pub const MIN_STRIP_INDICES: usize = 3;

/// One step of a decoded strip, in drawing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripTriangle {
    /// Strip step `k` (0 for the first triangle). Odd steps are the ones
    /// whose first two corners were swapped.
    pub step: usize,
    /// Positions in the strip of the three corners, in drawing order. This is
    /// the source-corner map for diagnostics.
    pub corners: [usize; 3],
    /// The indices at those positions, in drawing order.
    pub indices: [u32; 3],
}

impl StripTriangle {
    /// Two of the three indices are equal: the step draws nothing but still
    /// counts for parity.
    pub fn is_degenerate(&self) -> bool {
        let [a, b, c] = self.indices;
        a == b || b == c || a == c
    }
}

/// A strip that cannot be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripError {
    /// Fewer than [`MIN_STRIP_INDICES`] indices.
    TooShort {
        /// Indices supplied.
        indices: usize,
    },
}

impl StripError {
    /// Stable machine-matchable identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooShort { .. } => "strip_too_short",
        }
    }
}

impl fmt::Display for StripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { indices } => write!(
                f,
                "{}: a triangle strip needs at least {MIN_STRIP_INDICES} indices, got {indices}",
                self.code()
            ),
        }
    }
}

impl std::error::Error for StripError {}

/// Decodes a strip into all of its steps, degenerate ones included.
///
/// Nothing is dropped: the result has exactly `indices.len() - 2` entries
/// and consumers filter with [`StripTriangle::is_degenerate`].
pub fn decode_strip(indices: &[u32]) -> Result<Vec<StripTriangle>, StripError> {
    if indices.len() < MIN_STRIP_INDICES {
        return Err(StripError::TooShort {
            indices: indices.len(),
        });
    }
    Ok((0..indices.len() - 2)
        .map(|step| {
            let corners = if step % 2 == 0 {
                [step, step + 1, step + 2]
            } else {
                [step + 1, step, step + 2]
            };
            StripTriangle {
                step,
                corners,
                indices: corners.map(|corner| indices[corner]),
            }
        })
        .collect())
}
