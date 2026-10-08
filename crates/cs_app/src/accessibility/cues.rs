//! Objective status cues that never rely on colour (F52-A, AC02).
//!
//! Non-negotiable behavior 2: objective status is never red/green alone. A
//! [`Cue`] always carries a [`Shape`] and a text key; the colour role is a
//! redundant extra, and the colour filter in the settings can change only that
//! role. [`cue_for`] is a pure function of the status, so no setting can make
//! two statuses look the same. [`Scaled`] applies the UI scale to the cue's
//! metrics so the text stays readable at the largest scale.
//!
//! The text keys are `cs_content::localization` keys to be resolved by F51;
//! the objective states themselves are designed, not the original's.

use cs_content::settings::{ColourFilter, Presentation};

/// An objective's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectiveStatus {
    /// Not yet started.
    Pending,
    /// In progress.
    Active,
    /// Done.
    Completed,
    /// Lost.
    Failed,
    /// Shown as optional; never gates mission success.
    Optional,
    /// Replaced by another objective.
    Superseded,
}

impl ObjectiveStatus {
    /// Every status.
    pub const ALL: [Self; 6] = [
        Self::Pending,
        Self::Active,
        Self::Completed,
        Self::Failed,
        Self::Optional,
        Self::Superseded,
    ];
}

/// A glyph shape, distinct per status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    /// An empty circle.
    Circle,
    /// A right-pointing arrow.
    Arrow,
    /// A tick.
    Check,
    /// A cross.
    Cross,
    /// A diamond.
    Diamond,
    /// A struck-through dash.
    Strike,
}

/// A redundant colour role; absent under a filter that removes colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColourRole {
    /// Neutral.
    Neutral,
    /// Attention.
    Attention,
    /// Success.
    Success,
    /// Failure.
    Failure,
    /// De-emphasised.
    Muted,
}

/// What an objective line shows for a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cue {
    /// The glyph.
    pub shape: Shape,
    /// The localization key of the status word.
    pub text_key: &'static str,
    /// The colour role, if the filter leaves colour.
    pub colour: Option<ColourRole>,
}

/// The cue for a status under a colour filter.
#[must_use]
pub fn cue_for(status: ObjectiveStatus, filter: ColourFilter) -> Cue {
    let (shape, text_key, role) = match status {
        ObjectiveStatus::Pending => (Shape::Circle, "objective.pending", ColourRole::Neutral),
        ObjectiveStatus::Active => (Shape::Arrow, "objective.active", ColourRole::Attention),
        ObjectiveStatus::Completed => (Shape::Check, "objective.completed", ColourRole::Success),
        ObjectiveStatus::Failed => (Shape::Cross, "objective.failed", ColourRole::Failure),
        ObjectiveStatus::Optional => (Shape::Diamond, "objective.optional", ColourRole::Neutral),
        ObjectiveStatus::Superseded => (Shape::Strike, "objective.superseded", ColourRole::Muted),
    };
    Cue {
        shape,
        text_key,
        colour: (filter != ColourFilter::Monochrome).then_some(role),
    }
}

/// A pixel metric scaled by the UI scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scaled {
    /// Glyph side in pixels.
    pub glyph_px: u32,
    /// Text height in pixels.
    pub text_px: u32,
}

/// Designed base glyph side at 100 %.
pub const BASE_GLYPH_PX: u32 = 16;
/// Designed base text height at 100 %.
pub const BASE_TEXT_PX: u32 = 14;

/// The cue metrics at the presentation's UI scale, rounded up so a scaled-down
/// value never shrinks below the base.
#[must_use]
pub fn scaled(presentation: &Presentation) -> Scaled {
    let scale = u32::from(presentation.ui_scale_percent);
    Scaled {
        glyph_px: (BASE_GLYPH_PX * scale).div_ceil(100),
        text_px: (BASE_TEXT_PX * scale).div_ceil(100),
    }
}
