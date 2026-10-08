//! The objectives page drawn on a real GPU (F52-D, the `gpu` stage of F52).
//!
//! Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! stage `### F52-D`. Shared contracts: `docs/contracts/UI-NETWORK.md` and
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! F52-B lays the page out ([`super::objective_page`]) and F52-C hands that
//! layout the live session's presentation, but nothing drew it — F52-B's
//! finding recorded "nothing here draws" as a limit resolving with F52-D's
//! `gpu` capability. This module is that half:
//!
//! * [`objective_page_boxes`] maps an [`ObjectivePage`] to the quads a capture
//!   frame shows, purely, so the geometry is testable without an adapter.
//! * [`capture_objective_page`] draws that frame on the real renderer through
//!   the production F51-D capture path ([`crate::text::capture_text_boxes`]),
//!   which refuses a frame that drew nothing and leaves no PNG behind.
//!
//! # What this is evidence of
//!
//! Two quads per intersecting row: the row's **band** (its own
//! [`ObjectiveLine::top_px`] / [`ObjectiveLine::height_px`] at the UI scale)
//! and its cue's **glyph box** (the scaled glyph side). The frame therefore
//! shows the page's measured row geometry at the chosen scale, which rows the
//! viewport fits, and which colour roles survived the filter.
//!
//! [`ObjectiveLine::top_px`]: super::objective_page::ObjectiveLine::top_px
//! [`ObjectiveLine::height_px`]: super::objective_page::ObjectiveLine::height_px
//!
//! # What this is **not**
//!
//! * **Not glyph rendering, not the cue's shape.** The row's status word needs
//!   F51's layout and a catalogue entry that the `objective.*` keys do not
//!   have yet, and a sprite is a rectangle, so the cue is drawn as its
//!   measured *box*, not as its circle/arrow/tick. Shape and text remain the
//!   player-facing form (non-negotiable behavior 2) and are F46/F51's to draw.
//! * **Not the row's width.** [`ObjectivePage`] records no row width and no
//!   text extent, so the band runs from the end of the cue column to the
//!   page's right edge — the frame's edge while the page fits the frame
//!   1:1, and uniformly short of it when [`page_scale`] scales the page
//!   down. That span is a frame choice, documented here, not a
//!   measurement; inventing a text width would be a guess.
//! * **Not the scrolled page.** [`ObjectivePage`] carries no scroll state, so
//!   the frame shows the page from its own top: `scroll_to_show` and
//!   `max_scroll_px` are what the display does with the page, and none of
//!   that reaches a capture.
//! * **Not the original's appearance.** Row geometry is designed (F52-B), no
//!   original pixel and no original option is read, and the colours below are
//!   authored for this witness. No original executable runs here.
//!
//! The colour is the cue's *redundant* [`ColourRole`]: under `monochrome`
//! every role is gone and every glyph box goes grey, while every box's
//! position and size are unchanged. That is the **colour-independence half**
//! of non-negotiable behavior 2 at the pixel level — colour never moves or
//! resizes a row. It is not the whole of behavior 2: the shape and text
//! alternatives the player actually reads are F46/F51's to draw (above).

use std::path::Path;

use super::cues::ColourRole;
use super::objective_page::ObjectivePage;
use crate::text::{
    TEXT_CAPTURE_HEIGHT, TEXT_CAPTURE_WIDTH, TextBox, TextCapture, TextCaptureError,
    capture_text_boxes,
};

/// The capture frame's width in pixels: the F51-D frame, so the two witnesses
/// read the same.
pub const PAGE_CAPTURE_WIDTH: u32 = TEXT_CAPTURE_WIDTH;

/// The capture frame's height in pixels. As [`PAGE_CAPTURE_WIDTH`].
pub const PAGE_CAPTURE_HEIGHT: u32 = TEXT_CAPTURE_HEIGHT;

/// The row band's fill colour: clearly off the frame's clear colour, so a
/// band that was drawn counts as covered.
const BAND_COLOR: [f32; 4] = [0.16, 0.19, 0.24, 1.0];

/// The glyph box's fill when the filter left no colour role.
const NO_ROLE_COLOR: [f32; 4] = [0.62, 0.64, 0.66, 1.0];

/// The authored colour of one redundant colour role.
///
/// Designed for this witness and never load-bearing: [`objective_page_boxes`]
/// places the same box either way, so a role only changes what the box is
/// filled with.
#[must_use]
pub const fn role_colour(role: ColourRole) -> [f32; 4] {
    match role {
        ColourRole::Neutral => NO_ROLE_COLOR,
        ColourRole::Attention => [0.95, 0.75, 0.25, 1.0],
        ColourRole::Success => [0.35, 0.8, 0.4, 1.0],
        ColourRole::Failure => [0.9, 0.3, 0.3, 1.0],
        ColourRole::Muted => [0.4, 0.42, 0.45, 1.0],
    }
}

/// How many page pixels one frame pixel stands for: 1:1 while the page's
/// viewport fits the frame, uniformly smaller when it does not.
///
/// The page's coordinate system is the viewport's own (`0..viewport_px`, y
/// down), anchored at the frame's top as an objectives page is at the top of
/// the screen. `None` for an empty viewport, which has nothing to draw.
fn page_scale(page: &ObjectivePage) -> Option<f32> {
    if page.viewport_px == 0 {
        return None;
    }
    Some((f64::from(PAGE_CAPTURE_HEIGHT) / f64::from(page.viewport_px)).min(1.0) as f32)
}

/// The quads the capture frame shows for `page`, in page order.
///
/// Every row that intersects the viewport contributes its band, clipped to the
/// viewport, and its cue's glyph box, likewise clipped: the frame shows
/// exactly the part of each row a player would see. A row is never moved or
/// trimmed to make it fit, and no row is ever resized on its own — when the
/// viewport does not fit the frame, [`page_scale`] scales **every** row
/// together by one uniform factor instead. A page with no visible line —
/// every row hidden or the viewport empty — yields no box, which is what
/// makes [`capture_objective_page`] refuse rather than write an empty
/// picture.
///
/// The source rectangle is the viewport: [`PAGE_CAPTURE_WIDTH`] page pixels
/// wide (the frame's own width; the page records no row width) by
/// `viewport_px` down. A box's x is measured from the frame's centre and its
/// y from the frame's top, so the page's first row lands at the frame's top.
/// The page's right edge — and so the band's right edge, which nothing else
/// bounds — reaches the frame's right edge only at 1:1; a scaled-down page is
/// centred and ends short of both frame edges by the same factor.
#[must_use]
pub fn objective_page_boxes(page: &ObjectivePage) -> Vec<TextBox> {
    let Some(scale) = page_scale(page) else {
        return Vec::new();
    };
    let frame_w = PAGE_CAPTURE_WIDTH as f32;
    let frame_h = PAGE_CAPTURE_HEIGHT as f32;
    let half_w = frame_w * 0.5;
    let half_h = frame_h * 0.5;
    let viewport = page.viewport_px as f32;
    let glyph = page.metrics.glyph_px as f32;
    // The cue column: half a glyph of designed padding, then the glyph. The
    // row band starts where the cue ends, so the two quads never overlap and
    // the frame's drawing order never decides a pixel.
    let pad = glyph * 0.5;
    let band_from = pad + glyph;

    let centre_x = |page_x: f32| (page_x - half_w) * scale;
    let centre_y = |page_top: f32, height: f32| half_h - (page_top + height * 0.5) * scale;

    let mut boxes = Vec::new();
    for line in &page.lines {
        let top = line.top_px as f32;
        let height = line.height_px as f32;
        let row_bottom = (top + height).min(viewport);
        let band_top = top.max(0.0);
        if row_bottom <= band_top {
            // The row is entirely above or below the viewport.
            continue;
        }
        let band_height = (row_bottom - band_top) * scale;
        let band_width = (frame_w - band_from) * scale;
        if band_width > 0.0 && band_height > 0.0 {
            boxes.push(TextBox {
                center: [
                    centre_x((band_from + frame_w) * 0.5),
                    centre_y(band_top, row_bottom - band_top),
                ],
                size: [band_width, band_height],
                color: BAND_COLOR,
            });
        }
        // The glyph is centred in its own row band; only the part inside the
        // viewport is drawn, so a half-visible row shows a half box rather
        // than a moved one.
        let glyph_top = (top + (height - glyph) * 0.5).max(0.0);
        let glyph_bottom = (top + (height + glyph) * 0.5).min(viewport);
        if glyph_bottom <= glyph_top || glyph <= 0.0 {
            continue;
        }
        boxes.push(TextBox {
            center: [
                centre_x(pad + glyph * 0.5),
                centre_y(glyph_top, glyph_bottom - glyph_top),
            ],
            size: [glyph * scale, (glyph_bottom - glyph_top) * scale],
            color: match line.cue.colour {
                Some(role) => role_colour(role),
                None => NO_ROLE_COLOR,
            },
        });
    }
    boxes
}

/// Draws `page` on the real adapter and writes the frame's PNG.
///
/// The refusal set is the production capture path's: an empty box list never
/// starts a renderer, and a frame that came back uniform — nothing was drawn —
/// is reported and its PNG removed, so no file is ever evidence of a picture
/// that was not taken.
///
/// # Errors
///
/// [`TextCaptureError::NoVisibleLines`] for a page with nothing to draw,
/// [`TextCaptureError::NoScreenshotCaptured`],
/// [`TextCaptureError::UniformFrame`] or [`TextCaptureError::Io`].
pub fn capture_objective_page(
    label: &str,
    page: &ObjectivePage,
    png: &Path,
) -> Result<TextCapture, TextCaptureError> {
    let boxes = objective_page_boxes(page);
    capture_text_boxes(label, &boxes, png)
}
