//! The text application boundary: measurement, markup-aware wrapping, the
//! fit-or-scroll decision and the screen text pipeline (F51-A, F51-B).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stages `### F51-A` and `### F51-B`. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! This is where the declared localization contract
//! ([`cs_content::localization`]) meets a screen. It is deliberately **not** a
//! renderer and **not** a font reader:
//!
//! * [`metrics::TextMetrics`] is the *typed measurement input* — advance
//!   widths, line height, ascent and the font's declared glyph coverage at a
//!   pixel size. The original font format is still unmeasured, so the numbers
//!   stay a caller-supplied input and [`metrics::synthetic_monospace`] is a
//!   declared monospace stand-in for development, never a measurement of the
//!   original. A decode that fills this seam from an original font belongs to
//!   the retail-capable F51-D audit once that format is known.
//! * [`layout::layout_text`] is the whole of AC01 at this stage: it lays a
//!   validated [`MarkupDocument`] out inside a panel, keeps the text inside
//!   the panel's largest **free band** — the horizontal band the panel's
//!   required controls do not occupy — and reports whether the result fits or
//!   has to scroll. Nothing here draws anything, and no game state lives in
//!   this module.
//! * [`screen::layout_localized_text`] (F51-B) is the one production call a
//!   screen makes: it resolves a [`TextId`] through a [`LocaleChain`], parses
//!   the resolved row's text with the declared grammar, lays the parsed
//!   document out and returns a [`screen::ScreenText`]. Every refused control,
//!   absent glyph and unresolved substitution is a [`layout::LayoutDiagnostic`]
//!   a screen can show, and a miss is a named error rather than an empty box.
//!
//! [`MarkupDocument`]: cs_content::localization::MarkupDocument
//! [`TextId`]: cs_content::localization::TextId
//! [`LocaleChain`]: cs_content::localization::LocaleChain
//!
//! # Why the free band, not the panel
//!
//! AC01 is "long localized text fits or scrolls **without covering required
//! buttons**". A panel that reserves its bottom row for `Ok`/`Cancel` has two
//! bands: the free one above the buttons and the buttons themselves. Wrapping
//! into the whole panel and then clipping to the panel's height is what paints
//! the last line over the buttons, so the viewport is the *free band* and the
//! overflow is scrolled inside it ([`layout::TextLayout::covers`] answers
//! whether any placed line touches a required control, and
//! [`layout::TextLayout::scroll_offset_for_line`] clamps a scroll so the
//! focused line stays inside the viewport).
//!
//! # What is designed and what is unknown
//!
//! No original font is parsed, no original layout behaviour is reproduced, and
//! no original text metrics were measured. Wrapping is greedy word wrapping
//! with a hard character break for a token wider than the line, because a
//! translation's longest word is not known in advance and must never overflow
//! the band. The unknowns are recorded in
//! `docs/findings/2026-10-01-f51-a-locale-fallback-markup-and-font-provenance.md`.

pub mod layout;
pub mod metrics;
pub mod screen;

pub use layout::{
    ControlIdError, LaidOutLine, LayoutDiagnostic, LayoutError, LayoutRequest, RequiredControl,
    TextFit, TextLayout, layout_text,
};
pub use metrics::{TextMetrics, TextMetricsError, synthetic_monospace};
pub use screen::{ScreenText, ScreenTextError, ScreenTextRequest, layout_localized_text};

/// The substitution values a screen supplies for a localized string.
///
/// Re-exported from the content crate so a screen needs one import: a
/// localized string is a *template*, and the values that vary (a pilot name, a
/// count, a key binding) are supplied at layout time. A value the caller does
/// not supply renders as
/// [`cs_content::localization::UNRESOLVED_SUBSTITUTION`] and is reported,
/// never left as a silent empty gap.
pub use cs_content::localization::{SubstitutionId, SubstitutionTable, UNRESOLVED_SUBSTITUTION};
