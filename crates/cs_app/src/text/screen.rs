//! The screen text production path: resolve a localized id, parse its markup,
//! lay it out, and surface every problem (F51-B).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-B`, acceptance test AC02. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! F51-A owns the pieces: a [`TextCatalog`] resolves a [`TextId`] through a
//! [`LocaleChain`], [`parse_markup`] validates a resource string against a
//! [`MarkupGrammar`], and [`layout_text`] wraps a validated document in a
//! panel. What it deliberately did **not** have is the one production call a
//! screen makes, and that is what this module adds:
//! [`layout_localized_text`] resolves the id, parses the resolved row's text
//! with the declared grammar, lays the parsed document out and returns a
//! [`ScreenText`]. A caller can no longer lay a *raw* resource string out and
//! lose the markup diagnostics, and a miss is a named
//! [`ScreenTextError::Missing`] rather than an empty box.
//!
//! # Diagnostics are the point
//!
//! The stage's minimum scenario is "malformed markup and absent glyphs produce
//! visible diagnostics". Both are already computed by
//! [`parse_markup`](cs_content::localization::parse_markup) and by
//! [`TextMetrics`](super::TextMetrics)'s declared
//! [`GlyphCoverage`](cs_content::localization::GlyphCoverage); this path is the
//! one place that collects them for a screen:
//! [`ScreenText::diagnostics`] returns each one as a typed
//! [`LayoutDiagnostic`] and [`ScreenText::diagnostic_report`] renders them, so
//! a caller shows them instead of discovering them in a log.
//!
//! # What this stage does not do
//!
//! It does not open a file, load a font or draw anything. F51-C wires this path
//! into the real menus, HUD and subtitles and owns focus order; the original
//! control-markup delimiters and font metrics remain unmeasured and stay
//! caller-declared.

use bevy::math::Rect;
use cs_content::localization::{
    LocaleChain, LocaleId, MarkupDocument, MarkupGrammar, SubstitutionTable, TextCatalog, TextId,
    TextResolution, parse_markup,
};

use super::layout::{
    LayoutDiagnostic, LayoutError, LayoutRequest, RequiredControl, TextLayout, layout_text,
};
use super::metrics::TextMetrics;

/// Everything one screen text layout needs.
///
/// A screen supplies the catalog, the id it wants, its locale chain, the
/// declared markup grammar, the measured font, the substitution values, the
/// panel and the controls the text must not cover. The chain, grammar and
/// metrics are the caller's because the original locale list, control-markup
/// spelling and font metrics are unmeasured; this path asserts nothing about
/// them.
#[derive(Clone, Debug)]
pub struct ScreenTextRequest<'a> {
    /// The decoded localized strings.
    pub catalog: &'a TextCatalog,
    /// The string to resolve.
    pub id: &'a TextId,
    /// The selected locale first, then the explicit fallbacks.
    pub chain: &'a LocaleChain,
    /// The declared control-markup grammar the string is validated against.
    pub grammar: &'a MarkupGrammar,
    /// The measured font the text is drawn with.
    pub metrics: &'a TextMetrics,
    /// The substitution values the screen supplies.
    pub substitutions: &'a SubstitutionTable,
    /// The panel the text lives in.
    pub panel: Rect,
    /// The controls inside the panel the text must never cover.
    pub required: &'a [RequiredControl],
}

/// Why a screen text layout could not be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenTextError {
    /// No locale in the chain had a row for the id, so there is nothing to
    /// draw. `tried` names every locale the resolution walked, in order — a
    /// missing string is never silently an empty label.
    Missing {
        /// The requested string id.
        id: TextId,
        /// Every locale that was tried, in chain order.
        tried: Vec<LocaleId>,
    },
    /// The string resolved but its panel has nowhere to put the text.
    Layout(LayoutError),
}

impl std::fmt::Display for ScreenTextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { id, tried } => {
                let labels: Vec<&str> = tried.iter().map(LocaleId::as_str).collect();
                write!(f, "no locale answered {id} (tried {})", labels.join(", "))
            }
            Self::Layout(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScreenTextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Missing { .. } => None,
            Self::Layout(error) => Some(error),
        }
    }
}

/// One localized string, resolved, parsed and laid out, with every problem the
/// markup grammar and the font's glyph coverage found.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenText {
    id: TextId,
    locale: LocaleId,
    used_fallback: bool,
    document: MarkupDocument,
    layout: TextLayout,
}

impl ScreenText {
    /// The string's stable identity, independent of the locale that answered.
    #[must_use]
    pub fn id(&self) -> &TextId {
        &self.id
    }

    /// The locale whose row answered (the selected locale or a fallback).
    #[must_use]
    pub fn locale(&self) -> &LocaleId {
        &self.locale
    }

    /// Whether the answer came from a fallback locale rather than the selected
    /// one.
    #[must_use]
    pub fn used_fallback(&self) -> bool {
        self.used_fallback
    }

    /// The parsed document, with its token stream and markup issues.
    #[must_use]
    pub fn document(&self) -> &MarkupDocument {
        &self.document
    }

    /// The laid-out lines, viewport and fit.
    #[must_use]
    pub fn layout(&self) -> &TextLayout {
        &self.layout
    }

    /// Every problem the layout found: refused markup, absent glyphs,
    /// unresolved substitutions and broken tokens.
    #[must_use]
    pub fn diagnostics(&self) -> &[LayoutDiagnostic] {
        self.layout.diagnostics()
    }

    /// Whether any diagnostic has to be shown.
    #[must_use]
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics().is_empty()
    }

    /// The stable codes of every diagnostic, in order.
    #[must_use]
    pub fn diagnostic_codes(&self) -> Vec<&str> {
        self.diagnostics()
            .iter()
            .map(LayoutDiagnostic::code)
            .collect()
    }

    /// Every diagnostic rendered as one line, so a screen or a log shows what
    /// is wrong instead of only that something is.
    #[must_use]
    pub fn diagnostic_report(&self) -> String {
        self.diagnostics()
            .iter()
            .map(LayoutDiagnostic::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Resolves a localized id, parses the resolved row's text against the declared
/// grammar and lays it out in the panel, surfacing every problem.
///
/// # Errors
///
/// [`ScreenTextError::Missing`] when no locale in the chain has the id, and
/// [`ScreenTextError::Layout`] when the panel has nowhere to put the text.
pub fn layout_localized_text(
    request: &ScreenTextRequest<'_>,
) -> Result<ScreenText, ScreenTextError> {
    let (locale, used_fallback, text) = match request.catalog.resolve(request.id, request.chain) {
        TextResolution::Resolved {
            row,
            locale,
            used_fallback,
            ..
        } => (locale, used_fallback, row.text()),
        TextResolution::Missing { id, tried } => {
            return Err(ScreenTextError::Missing { id, tried });
        }
    };

    let document = parse_markup(text, request.grammar);
    let layout = layout_text(&LayoutRequest {
        id: Some(request.id.clone()),
        document: &document,
        metrics: request.metrics,
        substitutions: request.substitutions,
        panel: request.panel,
        required: request.required,
    })
    .map_err(ScreenTextError::Layout)?;

    Ok(ScreenText {
        id: request.id.clone(),
        locale,
        used_fallback,
        document,
        layout,
    })
}
