//! Authored front-end screen layout: logical hotspots and the aspect-fit
//! transform shared with the image they sit on (F45-A).
//!
//! Spec: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! stage `### F45-A`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Authored
//! images are aspect-fit with logical hotspot coordinates transformed by the
//! same scale/offset as the image").
//!
//! A [`ScreenLayout`] declares the logical size of an authored image and the
//! hotspots drawn on it, in that logical space. [`AspectFit`] fits the image
//! into a surface at its own aspect ratio, centred and never stretched, and
//! maps every hotspot with the *same* scale and offset, in integer arithmetic
//! so the result is the same on every machine. A surface point outside the
//! fitted image hits nothing.
//!
//! A hotspot names the action its button requests by key; this crate does not
//! know the front-end vocabulary, so `cs_app::ui::front_end::check_layout`
//! verifies every key against the state table. No original coordinate is
//! recorded here: the original hotspot values are F45-B's to import.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};

/// An axis-aligned rectangle; the unit is logical pixels of the authored image
/// or surface pixels of a fitted one, as the owner documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Rect {
    /// Whether the point lies inside (right/bottom edges exclusive).
    #[must_use]
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && y >= self.y
            && u64::from(x) < u64::from(self.x) + u64::from(self.width)
            && u64::from(y) < u64::from(self.y) + u64::from(self.height)
    }
}

/// One button region on an authored image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotspot {
    /// The stable `ui-resource` id of the button.
    pub id: ContentId,
    /// The key of the front-end action the button requests.
    pub action: String,
    /// The region in logical image coordinates.
    pub rect: Rect,
}

/// Why a layout was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The logical image size has a zero side.
    EmptyImage,
    /// A hotspot id is not a `ui-resource` id.
    WrongKind {
        /// The offending id.
        id: ContentId,
    },
    /// Two hotspots share an id.
    DuplicateHotspot {
        /// The repeated id.
        id: ContentId,
    },
    /// A hotspot has no area or leaves the logical image.
    OutOfBounds {
        /// The offending id.
        id: ContentId,
    },
    /// A hotspot names an empty action key.
    EmptyAction {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImage => write!(f, "the logical image size has a zero side"),
            Self::WrongKind { id } => write!(f, "hotspot {id} is not a ui-resource id"),
            Self::DuplicateHotspot { id } => write!(f, "hotspot {id} is declared twice"),
            Self::OutOfBounds { id } => {
                write!(f, "hotspot {id} is empty or leaves the logical image")
            }
            Self::EmptyAction { id } => write!(f, "hotspot {id} names no action"),
        }
    }
}

impl std::error::Error for LayoutError {}

/// A validated screen: a logical image size and its hotspots in declaration
/// order (the order keyboard/controller focus visits them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenLayout {
    image: (u32, u32),
    hotspots: Vec<Hotspot>,
}

impl ScreenLayout {
    /// Validates and builds a layout.
    ///
    /// # Errors
    ///
    /// [`LayoutError`] for the first empty image, wrong-kind, duplicate,
    /// out-of-bounds or action-less hotspot.
    pub fn new(image: (u32, u32), hotspots: Vec<Hotspot>) -> Result<Self, LayoutError> {
        if image.0 == 0 || image.1 == 0 {
            return Err(LayoutError::EmptyImage);
        }
        let mut seen = BTreeSet::new();
        for hotspot in &hotspots {
            let id = hotspot.id.clone();
            if id.kind() != ContentKind::UiResource {
                return Err(LayoutError::WrongKind { id });
            }
            if !seen.insert(id.clone()) {
                return Err(LayoutError::DuplicateHotspot { id });
            }
            let rect = hotspot.rect;
            let inside = u64::from(rect.x) + u64::from(rect.width) <= u64::from(image.0)
                && u64::from(rect.y) + u64::from(rect.height) <= u64::from(image.1);
            if rect.width == 0 || rect.height == 0 || !inside {
                return Err(LayoutError::OutOfBounds { id });
            }
            if hotspot.action.is_empty() {
                return Err(LayoutError::EmptyAction { id });
            }
        }
        Ok(Self { image, hotspots })
    }

    /// The logical image size.
    #[must_use]
    pub fn image(&self) -> (u32, u32) {
        self.image
    }

    /// The hotspots in focus order.
    #[must_use]
    pub fn hotspots(&self) -> &[Hotspot] {
        &self.hotspots
    }

    /// The hotspot under a surface point once the image is fitted, if any.
    /// Later hotspots win an overlap, matching draw order.
    #[must_use]
    pub fn hit_test(&self, fit: &AspectFit, x: u32, y: u32) -> Option<&Hotspot> {
        self.hotspots
            .iter()
            .rev()
            .find(|hotspot| fit.map_rect(hotspot.rect).contains(x, y))
    }
}

/// The aspect-fit of a logical image into a surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AspectFit {
    logical: (u32, u32),
    fitted: (u32, u32),
    offset: (u32, u32),
}

impl AspectFit {
    /// Fits `logical` into `surface`: the largest size with the image's own
    /// aspect ratio (rounded down), centred. `None` when either size has a
    /// zero side.
    #[must_use]
    pub fn new(logical: (u32, u32), surface: (u32, u32)) -> Option<Self> {
        if logical.0 == 0 || logical.1 == 0 || surface.0 == 0 || surface.1 == 0 {
            return None;
        }
        let (lw, lh) = (u64::from(logical.0), u64::from(logical.1));
        let (sw, sh) = (u64::from(surface.0), u64::from(surface.1));
        let (fw, fh) = if sw * lh <= sh * lw {
            (sw, lh * sw / lw)
        } else {
            (lw * sh / lh, sh)
        };
        let to_u32 = |value: u64| u32::try_from(value).ok();
        Some(Self {
            logical,
            fitted: (to_u32(fw)?, to_u32(fh)?),
            offset: (to_u32((sw - fw) / 2)?, to_u32((sh - fh) / 2)?),
        })
    }

    /// The fitted image rectangle in surface pixels.
    #[must_use]
    pub fn image_rect(&self) -> Rect {
        Rect {
            x: self.offset.0,
            y: self.offset.1,
            width: self.fitted.0,
            height: self.fitted.1,
        }
    }

    /// Maps a logical rectangle into surface pixels with the image's own scale
    /// and offset. Edges are mapped, not sizes, so adjacent hotspots stay
    /// adjacent.
    #[must_use]
    pub fn map_rect(&self, rect: Rect) -> Rect {
        let edge = |logical: u64, fitted: u32, base: u32, span: u32| {
            let scaled = logical * u64::from(fitted) / u64::from(span);
            u32::try_from(scaled)
                .unwrap_or(u32::MAX)
                .saturating_add(base)
        };
        let left = edge(
            u64::from(rect.x),
            self.fitted.0,
            self.offset.0,
            self.logical.0,
        );
        let top = edge(
            u64::from(rect.y),
            self.fitted.1,
            self.offset.1,
            self.logical.1,
        );
        let right = edge(
            u64::from(rect.x) + u64::from(rect.width),
            self.fitted.0,
            self.offset.0,
            self.logical.0,
        );
        let bottom = edge(
            u64::from(rect.y) + u64::from(rect.height),
            self.fitted.1,
            self.offset.1,
            self.logical.1,
        );
        Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }
}
