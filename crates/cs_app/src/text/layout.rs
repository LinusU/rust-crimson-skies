//! The fit-or-scroll layout of one localized string in one panel (F51-A).
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-A`, acceptance test AC01. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! [`layout_text`] is the whole of AC01 at this stage. It takes a validated
//! [`MarkupDocument`](cs_content::localization::MarkupDocument), a
//! [`TextMetrics`](crate::text::TextMetrics), the panel rectangle and the
//! panel's required controls, and answers one question: **where may the text
//! go, and does it fit there?**
//!
//! * The viewport is the panel's largest *free band* — the tallest horizontal
//!   strip of the panel that no required control occupies. A panel with a
//!   bottom button row therefore gets the strip above the buttons, never the
//!   buttons' own rectangle.
//! * The document's hard line breaks are kept, and each resulting paragraph is
//!   greedily word-wrapped into the viewport width. A single token wider than
//!   the viewport is broken by character, so a long translation can never paint
//!   outside the band.
//! * When the wrapped text is taller than the viewport the result is
//!   [`TextFit::Scrolls`] with the content height and the hidden line count, and
//!   [`TextLayout::scroll_offset_for_line`] clamps a scroll so the focused line
//!   stays inside the viewport. Nothing is silently truncated away.
//! * Every absent glyph, every refused markup control and every unresolved
//!   substitution becomes a [`LayoutDiagnostic`], so "missing glyphs are
//!   counted, not silently invisible" is a report a screen can show.
//!
//! The result is data, not drawing: a renderer consumes it, and F51-C wires it
//! into the real menus, HUD and subtitles.

use bevy::math::Rect;
use cs_content::localization::{
    MarkupDocument, SubstitutionTable, TextId, UNRESOLVED_SUBSTITUTION,
};
use cs_types::content::{ContentId, ContentIdError, ContentKind};

use super::metrics::TextMetrics;

/// Why a [`RequiredControl`] was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlIdError {
    /// The content id was rejected by the shared id grammar.
    Content(ContentIdError),
    /// The id named a content kind other than `ui_resource`.
    ///
    /// A required control is a user-interface element, so it keeps a
    /// `ui_resource` identity. Spelling a mission or a sound as a button would
    /// let a locale-dependent lookup reach outside the UI (F51 non-negotiable
    /// behavior 5).
    NotAUiResource {
        /// The kind the id actually named.
        kind: ContentKind,
    },
}

impl std::fmt::Display for ControlIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Content(error) => write!(f, "{error}"),
            Self::NotAUiResource { kind } => {
                write!(f, "required control id names a {kind}, not a ui_resource")
            }
        }
    }
}

impl std::error::Error for ControlIdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Content(error) => Some(error),
            Self::NotAUiResource { .. } => None,
        }
    }
}

/// One control the text must never cover: a button, a menu row, a slider.
///
/// A required control is not optional decoration. AC01 is about these staying
/// readable and reachable no matter how long the translation is, so the layout
/// reserves their rectangles and lays the text out around them.
#[derive(Clone, Debug, PartialEq)]
pub struct RequiredControl {
    id: ContentId,
    rect: Rect,
}

impl RequiredControl {
    /// Wraps a `ui_resource` control id and the rectangle it occupies.
    ///
    /// # Errors
    ///
    /// [`ControlIdError::Content`] for an id the shared grammar rejects and
    /// [`ControlIdError::NotAUiResource`] for any other namespace.
    pub fn new(id: ContentId, rect: Rect) -> Result<Self, ControlIdError> {
        if id.kind() != ContentKind::UiResource {
            return Err(ControlIdError::NotAUiResource { kind: id.kind() });
        }
        Ok(Self { id, rect })
    }

    /// The control's content id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// The rectangle the control occupies, in the panel's coordinate space.
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.rect
    }
}

/// Everything one layout needs.
#[derive(Clone, Debug)]
pub struct LayoutRequest<'a> {
    /// The string's identity, when the caller knows it, so a diagnostic about
    /// this layout names the string rather than a rectangle.
    pub id: Option<TextId>,
    /// The validated localized string.
    pub document: &'a MarkupDocument,
    /// The measured font the text is drawn with.
    pub metrics: &'a TextMetrics,
    /// The substitution values the screen supplies.
    pub substitutions: &'a SubstitutionTable,
    /// The panel the text lives in.
    pub panel: Rect,
    /// The controls inside the panel the text must never cover.
    pub required: &'a [RequiredControl],
}

/// Why a layout could not be produced at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// The panel rectangle is empty: there is nowhere to draw anything.
    EmptyPanel,
    /// Every part of the panel is covered by a required control, so there is no
    /// free band to lay text out in.
    NoFreeBand,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPanel => f.write_str("the panel rectangle is empty"),
            Self::NoFreeBand => f.write_str(
                "every part of the panel is covered by a required control, leaving no free band for text",
            ),
        }
    }
}

impl std::error::Error for LayoutError {}

/// Whether the laid-out text fits the viewport or has to scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextFit {
    /// Every line fits inside the viewport at scroll offset zero.
    Fits {
        /// How many lines the text produced.
        lines: usize,
    },
    /// The text is taller than the viewport and must be scrolled inside it.
    Scrolls {
        /// How many lines the text produced.
        lines: usize,
        /// How many lines lie completely outside the viewport.
        hidden_lines: usize,
        /// The height of the whole text, in pixels.
        content_height: f32,
        /// The height of the viewport, in pixels.
        viewport_height: f32,
    },
}

impl TextFit {
    /// Whether the text fits without scrolling.
    #[must_use]
    pub fn is_fits(&self) -> bool {
        matches!(self, Self::Fits { .. })
    }

    /// Whether the text must be scrolled.
    #[must_use]
    pub fn is_scrolling(&self) -> bool {
        matches!(self, Self::Scrolls { .. })
    }

    /// How many lines the text produced.
    #[must_use]
    pub fn lines(&self) -> usize {
        match self {
            Self::Fits { lines } | Self::Scrolls { lines, .. } => *lines,
        }
    }
}

/// One laid-out line: its text, its index in reading order, and where it sits
/// in the panel.
///
/// `rect` spans the **whole viewport width** for the line's height, because
/// that is the box a renderer fills. Keeping the conservative width is what
/// makes [`TextLayout::covers`] a real check: a renderer that paints the line
/// box cannot paint outside it. [`LaidOutLine::text_width`] is the narrower
/// measured extent of the line's own text, which a renderer or a geometry
/// witness can use when it wants to show the text's extent rather than the
/// band's.
#[derive(Clone, Debug, PartialEq)]
pub struct LaidOutLine {
    text: String,
    index: usize,
    rect: Rect,
    baseline: f32,
    text_width: f32,
}

impl LaidOutLine {
    /// The line's text, with the hard line breaks already applied.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The line's index in reading order, counted from the first line.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The line's box in panel coordinates. A scrolled line's box may lie
    /// outside the viewport; it is only painted where it is clipped to the
    /// viewport, which is what [`TextLayout::painted_rect`] returns.
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// The measured advance width of this line's text, in pixels.
    ///
    /// Never wider than [`LaidOutLine::rect`]: wrapping only breaks a line when
    /// the next word would exceed the band, so the text's extent stays inside
    /// the conservative box.
    #[must_use]
    pub fn text_width(&self) -> f32 {
        self.text_width
    }

    /// The baseline's `y`, in panel coordinates.
    #[must_use]
    pub fn baseline(&self) -> f32 {
        self.baseline
    }
}

/// One problem the layout found, in a form a screen can display.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutDiagnostic {
    /// A control, substitution or delimiter the markup grammar refused.
    Markup {
        /// The stable issue label from the content crate.
        code: String,
        /// The byte offset in the localized string, when the issue has one.
        offset: Option<usize>,
        /// The human-readable detail.
        detail: String,
    },
    /// A character the font has no glyph for, counted rather than hidden.
    MissingGlyph {
        /// The character.
        ch: char,
        /// The line it appeared on.
        line: usize,
        /// How often it appeared on that line.
        occurrences: usize,
    },
    /// A substitution the screen did not supply a value for. It renders as the
    /// visible [`UNRESOLVED_SUBSTITUTION`] marker.
    UnresolvedSubstitution {
        /// The substitution id.
        id: String,
        /// The line the marker landed on, when it landed on one.
        line: Option<usize>,
    },
    /// A single token was wider than the viewport and had to be broken by
    /// character, so it can never paint outside the band.
    BrokenWord {
        /// The first line the broken token starts on.
        line: usize,
    },
}

impl LayoutDiagnostic {
    /// A stable, machine-matchable label for the diagnostic.
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::Markup { code, .. } => code,
            Self::MissingGlyph { .. } => "missing_glyph",
            Self::UnresolvedSubstitution { .. } => "unresolved_substitution",
            Self::BrokenWord { .. } => "broken_word",
        }
    }
}

impl std::fmt::Display for LayoutDiagnostic {
    /// A one-line, human-readable rendering, so a screen can show every
    /// diagnostic rather than only count it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Markup {
                code,
                offset,
                detail,
            } => match offset {
                Some(offset) => write!(f, "{code}: {detail} at byte {offset}"),
                None => write!(f, "{code}: {detail}"),
            },
            Self::MissingGlyph {
                ch,
                line,
                occurrences,
            } => write!(
                f,
                "missing_glyph: the font has no glyph for '{ch}' on line {line} ({occurrences} occurrence(s))"
            ),
            Self::UnresolvedSubstitution { id, line } => match line {
                Some(line) => write!(f, "unresolved_substitution: {{{id}}} on line {line}"),
                None => write!(f, "unresolved_substitution: {{{id}}}"),
            },
            Self::BrokenWord { line } => write!(
                f,
                "broken_word: a token wider than the viewport was broken at line {line}"
            ),
        }
    }
}

/// The result of laying one localized string out in one panel.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    id: Option<TextId>,
    panel: Rect,
    viewport: Rect,
    line_height: f32,
    lines: Vec<LaidOutLine>,
    fit: TextFit,
    diagnostics: Vec<LayoutDiagnostic>,
}

impl TextLayout {
    /// The string this layout belongs to, when the caller supplied it.
    #[must_use]
    pub fn id(&self) -> Option<&TextId> {
        self.id.as_ref()
    }

    /// The panel the text was laid out in.
    #[must_use]
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// The free band the text is laid out in: the tallest horizontal strip of
    /// the panel that no required control occupies.
    #[must_use]
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Every line, in reading order, with its box in panel coordinates.
    #[must_use]
    pub fn lines(&self) -> &[LaidOutLine] {
        &self.lines
    }

    /// Whether the text fits or scrolls.
    #[must_use]
    pub fn fit(&self) -> TextFit {
        self.fit
    }

    /// The height of one line, in pixels.
    #[must_use]
    pub fn line_height(&self) -> f32 {
        self.line_height
    }

    /// Every problem the layout found.
    #[must_use]
    pub fn diagnostics(&self) -> &[LayoutDiagnostic] {
        &self.diagnostics
    }

    /// The indices of the lines that are painted at scroll offset zero, in
    /// reading order.
    #[must_use]
    pub fn visible_line_indices(&self) -> Vec<usize> {
        self.lines
            .iter()
            .filter(|line| !line.rect.intersect(self.viewport).is_empty())
            .map(LaidOutLine::index)
            .collect()
    }

    /// The box a renderer may paint for `index`, clipped to the viewport.
    ///
    /// A clipped line can never extend past the viewport, so it can never reach
    /// a control outside it. An index with no visible part returns `None`.
    #[must_use]
    pub fn painted_rect(&self, index: usize) -> Option<Rect> {
        let rect = self.lines.get(index)?.rect.intersect(self.viewport);
        (!rect.is_empty()).then_some(rect)
    }

    /// Whether any painted line covers `control`.
    ///
    /// This is the AC01 check, evaluated on the *clipped* line boxes: a long
    /// translation is laid out in the free band, so the answer is `false` for
    /// every required control however long the text is.
    #[must_use]
    pub fn covers(&self, control: &RequiredControl) -> bool {
        self.lines.iter().enumerate().any(|(index, _)| {
            self.painted_rect(index)
                .is_some_and(|painted| !painted.intersect(control.rect()).is_empty())
        })
    }

    /// How far the viewport may be scrolled: `0` when the text fits.
    #[must_use]
    pub fn max_scroll(&self) -> f32 {
        match self.fit {
            TextFit::Fits { .. } => 0.0,
            TextFit::Scrolls {
                content_height,
                viewport_height,
                ..
            } => (content_height - viewport_height).max(0.0),
        }
    }

    /// The scroll offset that brings `index` fully into the viewport.
    ///
    /// The result is always inside `0..=max_scroll()`, so a focus step can
    /// never scroll a line under a control or past the end of the text. An
    /// out-of-range index is clamped to the last line.
    #[must_use]
    pub fn scroll_offset_for_line(&self, index: usize) -> f32 {
        if self.lines.is_empty() {
            return 0.0;
        }
        let line = &self.lines[index.min(self.lines.len() - 1)];
        let top = line.rect.min.y - self.viewport.min.y;
        let wanted = (top + self.line_height - self.viewport.height()).max(0.0);
        wanted.min(self.max_scroll())
    }

    /// The text of every line, joined with newlines.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(LaidOutLine::text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Lays a validated localized string out in a panel, around its required
/// controls.
///
/// The document's hard line breaks are kept, the remaining text is greedily
/// word-wrapped into the free band, and the result reports whether it fits or
/// scrolls. Every problem the markup grammar or the font coverage found is
/// returned as a [`LayoutDiagnostic`] rather than being applied silently.
///
/// # Errors
///
/// [`LayoutError::EmptyPanel`] when the panel is empty, and
/// [`LayoutError::NoFreeBand`] when the required controls cover the whole
/// panel — which is a layout bug in the screen, reported instead of painting
/// over a control.
pub fn layout_text(request: &LayoutRequest<'_>) -> Result<TextLayout, LayoutError> {
    if request.panel.is_empty() {
        return Err(LayoutError::EmptyPanel);
    }
    let viewport = free_band(request.panel, request.required).ok_or(LayoutError::NoFreeBand)?;
    let width = viewport.width();

    let mut diagnostics: Vec<LayoutDiagnostic> = request
        .document
        .issues()
        .iter()
        .map(|issue| LayoutDiagnostic::Markup {
            code: issue.code().to_owned(),
            offset: issue.offset(),
            detail: issue.detail.clone(),
        })
        .collect();

    // Each paragraph is wrapped on its own, and the unresolved ids it carries
    // are consumed as the paragraph's markers are laid out, so a diagnostic
    // names the line its own marker landed on rather than the first line that
    // happens to hold one.
    let (paragraphs, per_paragraph_unresolved) = request
        .document
        .paragraph_substitutions(request.substitutions);
    let mut texts: Vec<String> = Vec::new();
    let mut unresolved: Vec<(String, Option<usize>)> = Vec::new();
    for (paragraph, ids) in paragraphs.iter().zip(&per_paragraph_unresolved) {
        let wrapped = wrap_paragraph(paragraph, width, request.metrics);
        if wrapped.broke_word {
            diagnostics.push(LayoutDiagnostic::BrokenWord { line: texts.len() });
        }
        // The k-th marker of a paragraph belongs to the k-th unresolved id of
        // that same paragraph: wrapping only reorders whole words, so a marker
        // character is never split across two lines.
        let mut pending: Vec<String> = ids.clone();
        for (offset, line) in wrapped.lines.iter().enumerate() {
            let mut on_this_line = line.matches(UNRESOLVED_SUBSTITUTION).count();
            while on_this_line > 0 {
                on_this_line -= 1;
                if let Some(id) = pending.first().cloned() {
                    pending.remove(0);
                    // A later occurrence of the same id does not report again;
                    // the first line the marker landed on is the one to show.
                    if !unresolved.iter().any(|(seen, _)| *seen == id) {
                        unresolved.push((id, Some(texts.len() + offset)));
                    }
                }
            }
        }
        // A marker the wrapper could not place (it cannot happen today, because
        // the marker is a single character and never whitespace) is still named.
        for id in pending {
            if !unresolved.iter().any(|(seen, _)| *seen == id) {
                unresolved.push((id, None));
            }
        }
        texts.extend(wrapped.lines);
    }

    let line_height = request.metrics.line_height();
    let ascent = request.metrics.ascent();
    let mut lines = Vec::with_capacity(texts.len());
    for (index, text) in texts.iter().enumerate() {
        let top = viewport.min.y + index as f32 * line_height;
        lines.push(LaidOutLine {
            text: text.clone(),
            index,
            rect: Rect::new(viewport.min.x, top, viewport.max.x, top + line_height),
            baseline: top + ascent,
            text_width: request.metrics.measure(text),
        });
        let report = request.metrics.coverage().missing_in(text);
        for ch in report.missing() {
            diagnostics.push(LayoutDiagnostic::MissingGlyph {
                ch: *ch,
                line: index,
                occurrences: report.count(*ch),
            });
        }
    }

    let content_height = lines.len() as f32 * line_height;
    let visible = lines
        .iter()
        .filter(|line| !line.rect.intersect(viewport).is_empty())
        .count();
    let fit = if content_height <= viewport.height() {
        TextFit::Fits { lines: lines.len() }
    } else {
        TextFit::Scrolls {
            lines: lines.len(),
            hidden_lines: lines.len() - visible,
            content_height,
            viewport_height: viewport.height(),
        }
    };

    for (id, line) in unresolved {
        diagnostics.push(LayoutDiagnostic::UnresolvedSubstitution { id, line });
    }

    Ok(TextLayout {
        id: request.id.clone(),
        panel: request.panel,
        viewport,
        line_height,
        lines,
        fit,
        diagnostics,
    })
}

/// The tallest horizontal strip of `panel` that no required control occupies.
///
/// A required control blocks a full-width band, because that is what a button
/// row in a dialog is: the control may be narrower than the panel, but the row
/// it sits in is the panel's, and text beside it would still collide with its
/// neighbours' focus order. Bands are compared by height and ties go to the
/// topmost band, so the choice is deterministic.
fn free_band(panel: Rect, required: &[RequiredControl]) -> Option<Rect> {
    let mut blockers: Vec<Rect> = required
        .iter()
        .map(RequiredControl::rect)
        .map(|rect| rect.intersect(panel))
        .filter(|rect| !rect.is_empty())
        .collect();
    blockers.sort_by(|left, right| {
        left.min
            .y
            .total_cmp(&right.min.y)
            .then(left.max.y.total_cmp(&right.max.y))
    });

    let mut bands: Vec<Rect> = Vec::new();
    let mut cursor = panel.min.y;
    for blocker in blockers {
        if blocker.min.y > cursor {
            bands.push(Rect::new(panel.min.x, cursor, panel.max.x, blocker.min.y));
        }
        cursor = cursor.max(blocker.max.y);
    }
    if panel.max.y > cursor {
        bands.push(Rect::new(panel.min.x, cursor, panel.max.x, panel.max.y));
    }
    bands.retain(|band| !band.is_empty());
    bands.into_iter().max_by(|left, right| {
        left.height()
            .total_cmp(&right.height())
            .then(right.min.y.total_cmp(&left.min.y))
    })
}

/// The wrapped lines of one paragraph, and whether a token had to be broken.
struct Wrapped {
    lines: Vec<String>,
    broke_word: bool,
}

/// Greedily word-wraps one paragraph into `width`.
///
/// A token wider than `width` is broken by character rather than allowed to
/// overflow: a translation's longest word is not knowable in advance, and an
/// overflow would paint over whatever sits next to the text.
fn wrap_paragraph(text: &str, width: f32, metrics: &TextMetrics) -> Wrapped {
    if text.trim().is_empty() {
        return Wrapped {
            lines: vec![String::new()],
            broke_word: false,
        };
    }
    let space = metrics.advance(' ');
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut broke_word = false;

    for word in text.split_whitespace() {
        if !current.is_empty() && metrics.measure(&current) + space + metrics.measure(word) > width
        {
            lines.push(std::mem::take(&mut current));
        }
        if current.is_empty() && metrics.measure(word) > width {
            broke_word = true;
            let mut piece = String::new();
            for ch in word.chars() {
                if !piece.is_empty() && metrics.measure(&piece) + metrics.advance(ch) > width {
                    lines.push(std::mem::take(&mut piece));
                }
                piece.push(ch);
            }
            if !piece.is_empty() {
                current = piece;
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    Wrapped { lines, broke_word }
}
