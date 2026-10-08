//! The objectives page as the player reads it (F52-B, AC02).
//!
//! [`objective_page`] is the consumer of the F46-C
//! [`PageView::Objectives`](crate::ui::hud::PageView::Objectives) rows: every
//! row gets its [`Cue`] for the colour filter and its metrics at the UI scale,
//! stacked top to bottom. A row's status comes from the objective runtime's own
//! state ([`status_of`]); no state shares a shape or a text key with another,
//! so the page reads the same with the colour filter on `Monochrome`.
//!
//! Nothing is ever dropped to make the page fit. At a large scale the content
//! outgrows the viewport and the page reports the scroll range, and
//! [`ObjectivePage::scroll_to_show`] brings any single line fully into view.
//!
//! Row geometry is designed (padding is [`ROW_GAP_PX`] at 100 %), not the
//! original's; the original objective page layout is F46-B/F47's.

use cs_content::settings::Presentation;
use cs_sim::objectives::state::ObjectiveState;
use cs_types::content::ContentId;

use super::cues::{Cue, ObjectiveStatus, Scaled, cue_for, scaled};
use crate::objectives::DisplayedObjective;
use crate::ui::hud::PageView;

/// Designed gap between two rows at 100 %.
pub const ROW_GAP_PX: u32 = 4;

/// The cue status of an objective state; `None` for [`ObjectiveState::Hidden`],
/// which the player is never shown.
#[must_use]
pub fn status_of(state: ObjectiveState) -> Option<ObjectiveStatus> {
    match state {
        ObjectiveState::Hidden => None,
        ObjectiveState::Pending => Some(ObjectiveStatus::Pending),
        ObjectiveState::Active => Some(ObjectiveStatus::Active),
        ObjectiveState::Succeeded => Some(ObjectiveStatus::Completed),
        ObjectiveState::Failed => Some(ObjectiveStatus::Failed),
        ObjectiveState::Optional => Some(ObjectiveStatus::Optional),
        ObjectiveState::Superseded => Some(ObjectiveStatus::Superseded),
    }
}

/// One objective line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveLine {
    /// The objective's content id, to resolve its description text.
    pub content: ContentId,
    /// The status shown.
    pub status: ObjectiveStatus,
    /// The cue for the colour filter.
    pub cue: Cue,
    /// Top edge below the page's top, in pixels.
    pub top_px: u32,
    /// Line height in pixels.
    pub height_px: u32,
}

impl ObjectiveLine {
    fn bottom_px(&self) -> u32 {
        self.top_px + self.height_px
    }
}

/// The page: lines at the UI scale in a viewport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectivePage {
    /// The lines, in the display's order.
    pub lines: Vec<ObjectiveLine>,
    /// The glyph and text metrics every line uses.
    pub metrics: Scaled,
    /// Height of the area the page is drawn in.
    pub viewport_px: u32,
}

impl ObjectivePage {
    /// Total height of all lines.
    #[must_use]
    pub fn content_px(&self) -> u32 {
        self.lines.last().map_or(0, ObjectiveLine::bottom_px)
    }

    /// The farthest the page can scroll; zero when everything fits.
    #[must_use]
    pub fn max_scroll_px(&self) -> u32 {
        self.content_px().saturating_sub(self.viewport_px)
    }

    /// The indices of lines that lie fully inside the viewport at `offset`
    /// (clamped to the scroll range).
    #[must_use]
    pub fn fully_visible(&self, offset: u32) -> Vec<usize> {
        let top = offset.min(self.max_scroll_px());
        let bottom = top + self.viewport_px;
        (0..self.lines.len())
            .filter(|&i| self.lines[i].top_px >= top && self.lines[i].bottom_px() <= bottom)
            .collect()
    }

    /// The scroll offset nearest to `from` that shows line `index` fully —
    /// smaller than `from` when the line already starts above the visible
    /// area — or `None` when there is no such line or it is taller than the
    /// viewport. The answer never exceeds the scroll range.
    #[must_use]
    pub fn scroll_to_show(&self, index: usize, from: u32) -> Option<u32> {
        let line = self.lines.get(index)?;
        if line.height_px > self.viewport_px {
            return None;
        }
        let from = from.min(self.max_scroll_px());
        let needed = line.bottom_px().saturating_sub(self.viewport_px);
        let offset = if line.top_px < from {
            line.top_px
        } else {
            from.max(needed)
        };
        Some(offset.min(self.max_scroll_px()))
    }
}

/// Lays out the visible objective rows.
///
/// A row whose state is not shown ([`status_of`] is `None`) is skipped; the
/// display never lists one, so skipping is a guard, not a filter.
#[must_use]
pub fn objective_page(
    rows: &[DisplayedObjective],
    presentation: &Presentation,
    viewport_px: u32,
) -> ObjectivePage {
    let metrics = scaled(presentation);
    let height_px = metrics.glyph_px.max(metrics.text_px);
    let gap = (ROW_GAP_PX * u32::from(presentation.ui_scale_percent)).div_ceil(100);
    let mut top_px = 0;
    let mut lines = Vec::new();
    for row in rows {
        let Some(status) = status_of(row.state) else {
            continue;
        };
        lines.push(ObjectiveLine {
            content: row.content.clone(),
            status,
            cue: cue_for(status, presentation.colour_filter),
            top_px,
            height_px,
        });
        top_px += height_px + gap;
    }
    ObjectivePage {
        lines,
        metrics,
        viewport_px,
    }
}

/// The page for a [`PageView::Objectives`]; `None` for any other page.
#[must_use]
pub fn from_view(
    view: &PageView,
    presentation: &Presentation,
    viewport_px: u32,
) -> Option<ObjectivePage> {
    match view {
        PageView::Objectives(rows) => Some(objective_page(rows, presentation, viewport_px)),
        _ => None,
    }
}
